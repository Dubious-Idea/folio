//! Создание, перенос, переименование и удаление страниц одного пространства.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use rayon::prelude::*;
use rustc_hash::FxHashSet;

use crate::error::{FolioError, IoContext, Result};
use crate::frontmatter::{FrontmatterWriter, PageDocument};
use crate::fsutil;
use crate::links::WikiLinkParser;
use crate::space::{LibrarySnapshot, SpaceSnapshot, uuid_string};
use crate::text::{PAGE_FILE_NAME, iso_date, make_page_id, natural_cmp, page_path, russian_date, title_key, validate_title};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameResult {
    pub new_path: String,
    /// Сколько файлов переписано: сама страница и страницы со ссылками на неё.
    pub updated_files: usize,
}

pub const TEMPLATES_FOLDER: &str = "_Шаблоны";
const MAXIMUM_FOLDER_NAME_LENGTH: usize = 120;

pub struct PageOperations<'a> {
    pub snapshot: &'a SpaceSnapshot,
}

impl<'a> PageOperations<'a> {
    pub fn new(snapshot: &'a SpaceSnapshot) -> Self {
        PageOperations { snapshot }
    }

    pub fn template_names(&self) -> Vec<String> {
        let folder = self.snapshot.space.root.join(TEMPLATES_FOLDER);
        let mut names: Vec<String> = fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let is_markdown = path.extension().is_some_and(|extension| extension == "md");
                is_markdown.then(|| path.file_stem()?.to_str().map(str::to_owned)).flatten()
            })
            .collect();
        names.sort_by(|lhs, rhs| natural_cmp(lhs, rhs));
        names
    }

    pub fn create_page(&self, raw_title: &str, parent_path: &str, template: Option<&str>, now: DateTime<Local>) -> Result<String> {
        let title = validate_title(raw_title)?;
        self.ensure_title_is_free(&title, None)?;
        if self.snapshot.page(parent_path).is_none() {
            let shown = if parent_path.is_empty() { self.snapshot.space.name() } else { parent_path };
            return Err(FolioError::PageNotFound(shown.to_owned()));
        }
        let content = self.page_content(&title, template, now)?;
        let parent_folder = self.snapshot.folder_path(parent_path);
        let folder_name = unique_folder_name(&folder_name(&title), &parent_folder, None);
        let folder = parent_folder.join(&folder_name);

        fs::create_dir(&folder).at(&folder)?;
        let file = folder.join(PAGE_FILE_NAME);
        fsutil::write_atomic(&file, content.as_bytes()).at(&file)?;
        Ok(page_path::join(parent_path, &folder_name))
    }

    pub fn move_page(&self, path: &str, parent_path: &str) -> Result<String> {
        if path.is_empty() {
            return Err(FolioError::HomePageIsFixed);
        }
        let page = self.snapshot.page(path).ok_or_else(|| FolioError::PageNotFound(path.to_owned()))?;
        if self.snapshot.page(parent_path).is_none() {
            return Err(FolioError::PageNotFound(parent_path.to_owned()));
        }
        if parent_path == path || parent_path.starts_with(&format!("{path}/")) {
            return Err(FolioError::MoveIntoItself);
        }
        if page.parent_path() == Some(parent_path) {
            return Ok(path.to_owned());
        }
        let destination_parent = self.snapshot.folder_path(parent_path);
        let name = unique_folder_name(page.folder_name(), &destination_parent, None);
        let source = self.snapshot.folder_path(path);
        let destination = destination_parent.join(&name);
        fs::rename(&source, &destination).at(&source)?;
        Ok(page_path::join(parent_path, &name))
    }

    /// Меняет заголовок, папку и все ссылки на страницу во всех пространствах библиотеки.
    pub fn rename_page(&self, path: &str, raw_title: &str, library: &LibrarySnapshot) -> Result<RenameResult> {
        let page = self.snapshot.page(path).ok_or_else(|| FolioError::PageNotFound(path.to_owned()))?;
        let title = validate_title(raw_title)?;
        self.ensure_title_is_free(&title, Some(path))?;
        if title == page.title {
            return Ok(RenameResult { new_path: path.to_owned(), updated_files: 0 });
        }

        // Ссылки переписываем до переноса папки, пока пути в снимке ещё верные.
        let mut updated = self.rewrite_links(&page.title, &title, library)?;
        self.rewrite_title(path, &title)?;
        updated.insert(format!("{}/{}", uuid_string(self.snapshot.space.id()), path));
        let updated_files = updated.len();

        if path.is_empty() {
            return Ok(RenameResult { new_path: String::new(), updated_files });
        }
        let desired = folder_name(&title);
        let mut new_path = path.to_owned();
        if desired != page.folder_name() {
            let parent_path = page.parent_path().unwrap_or("");
            let parent_folder = self.snapshot.folder_path(parent_path);
            let name = unique_folder_name(&desired, &parent_folder, Some(page.folder_name()));
            rename_folder(&self.snapshot.folder_path(path), &parent_folder.join(&name))?;
            new_path = page_path::join(parent_path, &name);
        }
        Ok(RenameResult { new_path, updated_files })
    }

    /// Папка страницы со всеми подстраницами уходит в Корзину.
    pub fn trash_page(&self, path: &str) -> Result<()> {
        if path.is_empty() {
            return Err(FolioError::HomePageIsFixed);
        }
        if self.snapshot.page(path).is_none() {
            return Err(FolioError::PageNotFound(path.to_owned()));
        }
        let folder = self.snapshot.folder_path(path);
        trash::delete(&folder).map_err(|error| FolioError::CommandFailed { command: "Корзина".to_owned(), message: error.to_string() })
    }

    fn ensure_title_is_free(&self, title: &str, except_path: Option<&str>) -> Result<()> {
        let clashes = self.snapshot.pages_titled(title).into_iter().any(|node| Some(node.path.as_str()) != except_path);
        if clashes { Err(FolioError::DuplicateTitle(title.to_owned())) } else { Ok(()) }
    }

    fn page_content(&self, title: &str, template: Option<&str>, now: DateTime<Local>) -> Result<String> {
        let mut extra_lines = Vec::new();
        let mut body = String::new();
        if let Some(template) = template {
            let file = self.snapshot.space.root.join(TEMPLATES_FOLDER).join(format!("{template}.md"));
            let text = fs::read_to_string(&file).map_err(|_| FolioError::TemplateNotFound(template.to_owned()))?;
            let rendered = text
                .replace("{{title}}", title)
                .replace("{{date}}", &russian_date(now))
                .replace("{{iso_date}}", &iso_date(now));
            let document = PageDocument::parse(&rendered);
            extra_lines = FrontmatterWriter::lines_excluding(document.raw_frontmatter.as_deref().unwrap_or(""), &["id", "title", "created"]);
            body = document.body;
        }
        let mut lines = vec![
            format!("id: {}", make_page_id()),
            format!("title: {}", FrontmatterWriter::scalar(title)),
            format!("created: {}", iso_date(now)),
        ];
        lines.extend(extra_lines);
        Ok(FrontmatterWriter::document(&lines, &body))
    }

    fn rewrite_title(&self, path: &str, title: &str) -> Result<()> {
        let file = self.snapshot.file_path(path);
        let text = fs::read_to_string(&file).at(&file)?;
        let document = PageDocument::parse(&text);
        let fallback = format!("id: {}", make_page_id());
        let lines = FrontmatterWriter::replacing_field("title", &FrontmatterWriter::scalar(title), document.raw_frontmatter.as_deref().unwrap_or(&fallback));
        let content = FrontmatterWriter::document(&lines, &document.body);
        fsutil::write_atomic(&file, content.as_bytes()).at(&file)
    }

    /// Возвращает ключи переписанных файлов вида `<id пространства>/<путь>`.
    /// Файлы читаются параллельно: на больших библиотеках это главное время переименования.
    fn rewrite_links(&self, old_title: &str, new_title: &str, library: &LibrarySnapshot) -> Result<FxHashSet<String>> {
        let old_key = title_key(old_title);
        let space_key = title_key(self.snapshot.space.name());
        let own_id = self.snapshot.space.id();
        let mut spaces: Vec<&SpaceSnapshot> = library.spaces.iter().map(|snapshot| snapshot.as_ref()).collect();
        if library.snapshot(own_id).is_none() {
            spaces.push(self.snapshot);
        }

        let jobs: Vec<(&SpaceSnapshot, &String)> = spaces.iter().flat_map(|other| other.pages.keys().map(move |path| (*other, path))).collect();
        let results: Vec<Result<Option<String>>> = jobs
            .par_iter()
            .map(|(other, path)| {
                let file = other.file_path(path);
                let Ok(text) = fs::read_to_string(&file) else { return Ok(None) };
                if !text.contains("[[") {
                    return Ok(None);
                }
                let is_same_space = other.space.id() == own_id;
                let rewritten = WikiLinkParser::rewrite(&text, new_title, |link| {
                    if title_key(&link.title) != old_key {
                        return false;
                    }
                    match &link.space {
                        Some(space) => title_key(space) == space_key,
                        None => is_same_space,
                    }
                });
                let Some(rewritten) = rewritten else { return Ok(None) };
                fsutil::write_atomic(&file, rewritten.as_bytes()).at(&file)?;
                Ok(Some(format!("{}/{}", uuid_string(other.space.id()), path)))
            })
            .collect();
        results.into_iter().filter_map(Result::transpose).collect()
    }
}

/// Имя папки из заголовка: без символов, запрещённых в путях Windows и macOS, и без служебных префиксов.
pub fn folder_name(title: &str) -> String {
    let replaced: String = title.chars().map(|character| if "/:\\<>\"|?*".contains(character) || character.is_control() { '-' } else { character }).collect();
    let trimmed = replaced.trim().trim_start_matches(['.', '_']);
    let limited: String = trimmed.chars().take(MAXIMUM_FOLDER_NAME_LENGTH).collect();
    // Windows не хранит точки и пробелы в конце имени.
    let limited = limited.trim().trim_end_matches(['.', ' ']);
    if limited.is_empty() {
        return "Без названия".to_owned();
    }
    if !is_reserved_device_name(limited) {
        return limited.to_owned();
    }
    // Зарезервировано имя до первой точки, поэтому дефис ставится перед расширением: «COM1-.txt».
    match limited.find('.') {
        Some(dot) => format!("{}-{}", &limited[..dot], &limited[dot..]),
        None => format!("{limited}-"),
    }
}

/// CON, PRN, AUX, NUL, COM1–9, LPT1–9: такие имена Windows не даёт создать.
fn is_reserved_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end().to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.chars().nth(3).is_some_and(|digit| digit.is_ascii_digit() && digit != '0'))
}

fn unique_folder_name(base: &str, parent: &Path, current: Option<&str>) -> String {
    let current = current.map(str::to_lowercase);
    let mut candidate = base.to_owned();
    let mut counter = 2;
    while Some(candidate.to_lowercase()) != current && parent.join(&candidate).exists() {
        candidate = format!("{base} {counter}");
        counter += 1;
    }
    candidate
}

/// На нечувствительном к регистру диске смена одного регистра идёт через временное имя.
fn rename_folder(source: &Path, destination: &Path) -> Result<()> {
    let same_name = source.file_name().map(|name| name.to_string_lossy().to_lowercase())
        == destination.file_name().map(|name| name.to_string_lossy().to_lowercase());
    if !same_name {
        return fs::rename(source, destination).at(source);
    }
    let temporary: PathBuf =
        source.with_file_name(format!("{}{}", fsutil::RENAME_PREFIX, uuid::Uuid::new_v4().simple()));
    fs::rename(source, &temporary).at(source)?;
    fs::rename(&temporary, destination).at(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::SpaceScanner;
    use crate::tests_support::Fixture;
    use std::sync::Arc;

    fn now() -> DateTime<Local> {
        Local::now()
    }

    #[test]
    fn creates_from_template() {
        let folder = tempfile::tempdir().unwrap();
        let space = Fixture::space("Работа", folder.path());
        let templates = space.root.join(TEMPLATES_FOLDER);
        fs::create_dir_all(&templates).unwrap();
        let template = "---\ntitle: Шаблон\nstatus: Черновик\nlabels: [встреча]\n---\n\n# {{title}}\n\nДата: {{date}}\n";
        fs::write(templates.join("Встреча.md"), template).unwrap();

        let snapshot = SpaceScanner::scan(&space);
        let operations = PageOperations::new(&snapshot);
        assert_eq!(operations.template_names(), ["Встреча"]);
        let path = operations.create_page("Встреча: заказчик", "", Some("Встреча"), now()).unwrap();

        assert_eq!(path, "Встреча- заказчик");
        let document = PageDocument::parse(&Fixture::read(&space, &path));
        assert_eq!(document.frontmatter.title.as_deref(), Some("Встреча: заказчик"));
        assert_eq!(document.frontmatter.status.as_ref().map(|status| status.label.as_str()), Some("Черновик"));
        assert_eq!(document.frontmatter.labels, ["встреча"]);
        assert_eq!(document.frontmatter.id.as_ref().map(String::len), Some(12));
        assert!(document.body.starts_with("# Встреча: заказчик\n\nДата: "));
        assert!(!document.body.contains("{{"));
    }

    #[test]
    fn rejects_invalid_titles() {
        let folder = tempfile::tempdir().unwrap();
        let space = Fixture::space("Работа", folder.path());
        Fixture::write_page(&space, "Требования", Some("Требования"), &[], "");
        let snapshot = SpaceScanner::scan(&space);
        let operations = PageOperations::new(&snapshot);

        assert!(matches!(operations.create_page("требования", "", None, now()), Err(FolioError::DuplicateTitle(title)) if title == "требования"));
        assert!(matches!(operations.create_page("Работа", "", None, now()), Err(FolioError::DuplicateTitle(title)) if title == "Работа"));
        assert!(matches!(operations.create_page("Плохой [заголовок]", "", None, now()), Err(FolioError::InvalidTitle { .. })));
    }

    #[test]
    fn moves_pages() {
        let folder = tempfile::tempdir().unwrap();
        let space = Fixture::space("Работа", folder.path());
        Fixture::write_page(&space, "Проекты", Some("Проекты"), &[], "");
        Fixture::write_page(&space, "Проекты/CRM", Some("CRM"), &[], "");
        Fixture::write_page(&space, "Архив", Some("Архив"), &[], "");
        let snapshot = SpaceScanner::scan(&space);
        let operations = PageOperations::new(&snapshot);

        assert!(matches!(operations.move_page("Проекты", "Проекты/CRM"), Err(FolioError::MoveIntoItself)));
        assert!(matches!(operations.move_page("", "Архив"), Err(FolioError::HomePageIsFixed)));
        assert_eq!(operations.move_page("Проекты/CRM", "Архив").unwrap(), "Архив/CRM");
        assert_eq!(SpaceScanner::scan(&space).page("Архив/CRM").map(|node| node.title.as_str()), Some("CRM"));
    }

    #[test]
    fn rename_rewrites_links_everywhere() {
        let folder = tempfile::tempdir().unwrap();
        let work = Fixture::space("Работа", folder.path());
        let personal = Fixture::space("Личное", folder.path());
        Fixture::write_page(&work, "Требования", Some("Требования"), &[], "Черновик");
        Fixture::write_page(&work, "Встреча", Some("Встреча"), &[], "См. [[требования|список]] и [[Работа::Требования]]");
        Fixture::write_page(&personal, "Заметка", Some("Заметка"), &[], "[[Работа::Требования]] и [[Требования]]");
        let library = LibrarySnapshot::new(vec![Arc::new(SpaceScanner::scan(&work)), Arc::new(SpaceScanner::scan(&personal))]);

        let result = PageOperations::new(&library.spaces[0]).rename_page("Требования", "Требования к CRM", &library).unwrap();

        assert_eq!(result, RenameResult { new_path: "Требования к CRM".to_owned(), updated_files: 3 });
        assert!(Fixture::read(&work, "Встреча").contains("См. [[Требования к CRM|список]] и [[Работа::Требования к CRM]]"));
        // Ссылка без пространства внутри «Личного» ведёт в «Личное», её не трогаем.
        assert!(Fixture::read(&personal, "Заметка").contains("[[Работа::Требования к CRM]] и [[Требования]]"));
        let renamed = PageDocument::parse(&Fixture::read(&work, "Требования к CRM"));
        assert_eq!(renamed.frontmatter.title.as_deref(), Some("Требования к CRM"));
    }

    #[test]
    fn case_only_rename_renames_folder() {
        let folder = tempfile::tempdir().unwrap();
        let space = Fixture::space("Работа", folder.path());
        Fixture::write_page(&space, "встреча", Some("встреча"), &[], "");
        let library = LibrarySnapshot::new(vec![Arc::new(SpaceScanner::scan(&space))]);

        let result = PageOperations::new(&library.spaces[0]).rename_page("встреча", "Встреча", &library).unwrap();

        assert_eq!(result.new_path, "Встреча");
        let names: Vec<String> = fs::read_dir(&space.root).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
        assert!(names.contains(&"Встреча".to_owned()));
        assert!(!names.contains(&"встреча".to_owned()));
    }

    #[test]
    fn folder_names_are_windows_safe() {
        assert_eq!(folder_name("Что? \"Да\" <или> нет*"), "Что- -Да- -или- нет-");
        assert_eq!(folder_name("Итоги..."), "Итоги");
        assert_eq!(folder_name("con"), "con-");
        assert_eq!(folder_name("COM1.txt"), "COM1-.txt");
        assert_eq!(folder_name("Console"), "Console");
        assert_eq!(folder_name("_.Скрытая"), "Скрытая");
        assert_eq!(folder_name("..."), "Без названия");
    }
}
