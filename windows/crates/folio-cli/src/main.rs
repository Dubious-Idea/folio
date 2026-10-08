//! Утилита `folio`: вики Folio из терминала. Команды и вывод те же, что у версии для macOS.

mod open;
mod workspace;

use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use chrono::Local;
use clap::{ArgAction, Args, Parser, Subcommand};
use folio_core::bootstrap::{SpaceKind, create_space};
use folio_core::checker;
use folio_core::git::GitRepository;
use folio_core::index::{self, HIGHLIGHT_END, HIGHLIGHT_START, MINIMUM_TERM_LENGTH, SearchIndex};
use folio_core::operations::PageOperations;
use folio_core::registry::{FolioPaths, SpaceRegistry};
use folio_core::text::{page_path, russian_date_time, title_key};
use folio_core::{FolioError, LibrarySnapshot, PageLink, PageNode, Space, SpaceSnapshot};

use workspace::{Failure, Workspace};

const HELP_TEMPLATE: &str = "{about-with-newline}\nИспользование: {usage}\n\n{all-args}{after-help}";

#[derive(Parser)]
#[command(
    name = "folio",
    bin_name = "folio",
    about = "Вики Folio из терминала: страницы, поиск, проверка и история.",
    version,
    help_template = HELP_TEMPLATE,
    subcommand_help_heading = "Команды",
    subcommand_value_name = "КОМАНДА",
    disable_help_flag = true,
    disable_version_flag = true,
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    #[arg(short, long, action = ArgAction::Help, help = "Показать справку", global = true, help_heading = "Параметры")]
    help: Option<bool>,
    #[arg(short = 'V', long, action = ArgAction::Version, help = "Показать версию", help_heading = "Параметры")]
    version: Option<bool>,
}

#[derive(Args)]
struct SpaceOption {
    /// Имя пространства. Внутри папки пространства можно не указывать.
    #[arg(short, long)]
    space: Option<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Подключённые пространства.
    #[command(help_template = HELP_TEMPLATE, next_help_heading = "Параметры")]
    Spaces,
    /// Создать пространство в пустой папке и подключить его.
    #[command(help_template = HELP_TEMPLATE, next_help_heading = "Параметры")]
    Init {
        /// Папка пространства.
        folder: String,
        /// Имя пространства.
        #[arg(long)]
        name: String,
        /// Тип: work, personal или blank.
        #[arg(long, default_value = "blank", value_parser = ["work", "personal", "blank"])]
        kind: String,
        /// Значок из SF Symbols.
        #[arg(long)]
        icon: Option<String>,
    },
    /// Подключить существующее пространство.
    #[command(help_template = HELP_TEMPLATE, next_help_heading = "Параметры")]
    Add {
        /// Папка пространства.
        folder: String,
    },
    /// Дерево страниц.
    #[command(help_template = HELP_TEMPLATE, next_help_heading = "Параметры")]
    Tree {
        #[command(flatten)]
        space: SpaceOption,
        /// Показать только ветку этой страницы.
        #[arg(long)]
        under: Option<String>,
        /// Сколько уровней показать.
        #[arg(long)]
        depth: Option<usize>,
        /// Показывать пути папок.
        #[arg(long)]
        paths: bool,
    },
    /// Путь к файлу страницы.
    #[command(help_template = HELP_TEMPLATE, next_help_heading = "Параметры")]
    Path {
        #[command(flatten)]
        space: SpaceOption,
        /// Заголовок страницы.
        title: String,
        /// Путь папки страницы вместо index.md.
        #[arg(long)]
        folder: bool,
    },
    /// Открыть страницу в приложении Folio.
    #[command(help_template = HELP_TEMPLATE, next_help_heading = "Параметры")]
    Open {
        #[command(flatten)]
        space: SpaceOption,
        /// Заголовок страницы; без него откроется главная пространства.
        title: Option<String>,
        /// Сразу открыть страницу на правку.
        #[arg(long)]
        edit: bool,
    },
    /// Создать страницу; печатает путь к index.md.
    #[command(help_template = HELP_TEMPLATE, next_help_heading = "Параметры")]
    New {
        #[command(flatten)]
        space: SpaceOption,
        /// Заголовок новой страницы.
        title: String,
        /// Заголовок родительской страницы; по умолчанию — главная.
        #[arg(long)]
        parent: Option<String>,
        /// Шаблон из папки _Шаблоны.
        #[arg(long)]
        template: Option<String>,
    },
    /// Переименовать страницу и починить ссылки на неё.
    #[command(help_template = HELP_TEMPLATE, next_help_heading = "Параметры")]
    Rename {
        #[command(flatten)]
        space: SpaceOption,
        /// Текущий заголовок.
        title: String,
        /// Новый заголовок.
        new_title: String,
    },
    /// Перенести страницу к другому родителю.
    #[command(help_template = HELP_TEMPLATE, next_help_heading = "Параметры")]
    Move {
        #[command(flatten)]
        space: SpaceOption,
        /// Заголовок страницы.
        title: String,
        /// Заголовок нового родителя; для верхнего уровня — имя пространства.
        #[arg(long)]
        parent: String,
    },
    /// Переместить страницу с подстраницами в Корзину.
    #[command(help_template = HELP_TEMPLATE, next_help_heading = "Параметры")]
    Delete {
        #[command(flatten)]
        space: SpaceOption,
        /// Заголовок страницы.
        title: String,
    },
    /// Полнотекстовый поиск по всем пространствам.
    #[command(help_template = HELP_TEMPLATE, next_help_heading = "Параметры")]
    Search {
        /// Искать только в этом пространстве.
        #[arg(short, long)]
        space: Option<String>,
        /// Что искать.
        query: String,
        /// Сколько результатов показать.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Страницы, которые ссылаются на эту.
    #[command(help_template = HELP_TEMPLATE, next_help_heading = "Параметры")]
    Backlinks {
        #[command(flatten)]
        space: SpaceOption,
        /// Заголовок страницы.
        title: String,
    },
    /// История правок страницы.
    #[command(help_template = HELP_TEMPLATE, next_help_heading = "Параметры")]
    History {
        #[command(flatten)]
        space: SpaceOption,
        /// Заголовок страницы.
        title: String,
        /// Сколько версий показать.
        #[arg(long, default_value_t = 30)]
        limit: usize,
    },
    /// Найти дубли заголовков, битые ссылки и потерянные вложения.
    #[command(help_template = HELP_TEMPLATE, next_help_heading = "Параметры")]
    Check {
        #[command(flatten)]
        space: SpaceOption,
        /// Проверить все пространства.
        #[arg(long)]
        all: bool,
    },
    /// Сохранить изменения пространства снимком в истории.
    #[command(help_template = HELP_TEMPLATE, next_help_heading = "Параметры")]
    Commit {
        #[command(flatten)]
        space: SpaceOption,
        /// Описание снимка.
        #[arg(short, long)]
        message: Option<String>,
    },
    /// Пересобрать индекс поиска с нуля.
    #[command(help_template = HELP_TEMPLATE, next_help_heading = "Параметры")]
    Reindex,
}

type Output<'a> = BufWriter<io::StdoutLock<'a>>;

fn main() -> ExitCode {
    let cli = Cli::parse();
    // Вывод копится и уходит в консоль одним куском: построчная запись в консоль Windows медленная.
    let mut out = BufWriter::new(io::stdout().lock());
    let result = run(cli.command, &mut out);
    let _ = out.flush();
    drop(out);
    match result {
        Ok(code) => code,
        Err(failure) => {
            eprintln!("Ошибка: {failure}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command, out: &mut Output<'_>) -> Result<ExitCode, Failure> {
    match command {
        Command::Spaces => spaces(out)?,
        Command::Init { folder, name, kind, icon } => init(&folder, &name, &kind, icon.as_deref(), out)?,
        Command::Add { folder } => {
            let space = Space::open(&expand_path(&folder))?;
            SpaceRegistry::default().register(&space)?;
            writeln!(out, "Подключено пространство «{}»", space.name())?;
        }
        Command::Tree { space, under, depth, paths } => {
            let workspace = Workspace::load();
            let snapshot = workspace.space(space.space.as_deref())?;
            let root = match &under {
                Some(title) => workspace.page(title, &snapshot)?,
                None => snapshot.home(),
            };
            TreePrinter { snapshot: &snapshot, shows_paths: paths, maximum_depth: depth }.render(root, out)?;
        }
        Command::Path { space, title, folder } => {
            let workspace = Workspace::load();
            let snapshot = workspace.space(space.space.as_deref())?;
            let page = workspace.page(&title, &snapshot)?;
            let path = if folder { snapshot.folder_path(&page.path) } else { snapshot.file_path(&page.path) };
            writeln!(out, "{}", display(&path))?;
        }
        Command::Open { space, title, edit } => {
            let workspace = Workspace::load();
            let snapshot = workspace.space(space.space.as_deref())?;
            let page = match &title {
                Some(title) => workspace.page(title, &snapshot)?,
                None => snapshot.home(),
            };
            open::open_url(&PageLink::url(&snapshot.page_ref(&page.path), edit))?;
        }
        Command::New { space, title, parent, template } => {
            let workspace = Workspace::load();
            let snapshot = workspace.space(space.space.as_deref())?;
            let parent_page = match &parent {
                Some(parent) => workspace.page(parent, &snapshot)?,
                None => snapshot.home(),
            };
            let path = PageOperations::new(&snapshot).create_page(&title, &parent_page.path, template.as_deref(), Local::now())?;
            writeln!(out, "{}", display(&snapshot.file_path(&path)))?;
        }
        Command::Rename { space, title, new_title } => {
            let workspace = Workspace::load();
            let snapshot = workspace.space(space.space.as_deref())?;
            let page = workspace.page(&title, &snapshot)?;
            let result = PageOperations::new(&snapshot).rename_page(&page.path, &new_title, &workspace.library())?;
            writeln!(out, "«{}» → «{new_title}», файлов обновлено: {}", page.title, result.updated_files)?;
            writeln!(out, "{}", display(&snapshot.file_path(&result.new_path)))?;
        }
        Command::Move { space, title, parent } => {
            let workspace = Workspace::load();
            let snapshot = workspace.space(space.space.as_deref())?;
            let page = workspace.page(&title, &snapshot)?;
            let parent_page = workspace.page(&parent, &snapshot)?;
            let new_path = PageOperations::new(&snapshot).move_page(&page.path, &parent_page.path)?;
            writeln!(out, "{}", display(&snapshot.file_path(&new_path)))?;
        }
        Command::Delete { space, title } => {
            let workspace = Workspace::load();
            let snapshot = workspace.space(space.space.as_deref())?;
            let page = workspace.page(&title, &snapshot)?;
            PageOperations::new(&snapshot).trash_page(&page.path)?;
            writeln!(out, "«{}» и её подстраницы ({}) перемещены в Корзину", page.title, page.children.len())?;
        }
        Command::Search { space, query, limit } => search(space.as_deref(), &query, limit, out)?,
        Command::Backlinks { space, title } => {
            let workspace = Workspace::load();
            let snapshot = workspace.space(space.space.as_deref())?;
            let page = workspace.page(&title, &snapshot)?;
            let hits = workspace.synced_index()?.backlinks(&page.title, &snapshot.space)?;
            if hits.is_empty() {
                writeln!(out, "На «{}» никто не ссылается", page.title)?;
            }
            for hit in hits {
                writeln!(out, "{} — {}", hit.title, workspace.describe(&hit.page))?;
            }
        }
        Command::History { space, title, limit } => {
            let workspace = Workspace::load();
            let snapshot = workspace.space(space.space.as_deref())?;
            let page = workspace.page(&title, &snapshot)?;
            let revisions = GitRepository::new(&snapshot.space.root).history(&page_path::page_file(&page.path), limit)?;
            if revisions.is_empty() {
                writeln!(out, "У «{}» пока нет сохранённых версий — сделай folio commit", page.title)?;
            }
            for revision in revisions {
                let date = russian_date_time(revision.date.with_timezone(&Local));
                writeln!(out, "{}  {date}  {}", revision.short_hash(), revision.message)?;
            }
        }
        Command::Check { space, all } => return check(space.space.as_deref(), all, out),
        Command::Commit { space, message } => {
            let snapshot = Workspace::load().space(space.space.as_deref())?;
            let repository = GitRepository::new(&snapshot.space.root);
            repository.initialize()?;
            let text = message.unwrap_or_else(|| format!("Правки от {}", russian_date_time(Local::now())));
            if repository.commit_all(&text)? {
                writeln!(out, "Снимок сохранён: {text}")?;
            } else {
                writeln!(out, "Изменений нет")?;
            }
        }
        Command::Reindex => {
            let base = FolioPaths::index_path();
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", base.display()));
            }
            let workspace = Workspace::load();
            let mut index = SearchIndex::open(&base)?;
            for snapshot in workspace.library().spaces {
                let count = index.sync(&snapshot)?;
                writeln!(out, "«{}»: проиндексировано страниц — {count}", snapshot.space.name())?;
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn spaces(out: &mut Output<'_>) -> Result<(), Failure> {
    let result = SpaceRegistry::default().load();
    if result.spaces.is_empty() && result.missing_paths.is_empty() {
        writeln!(out, "Пространств нет. Создай его: folio init <папка> --name <имя>")?;
        return Ok(());
    }
    for snapshot in LibrarySnapshot::scan(&result.spaces).spaces {
        writeln!(out, "{}\t{} стр.\t{}", snapshot.space.name(), snapshot.pages.len(), snapshot.space.root_path())?;
    }
    for path in &result.missing_paths {
        writeln!(out, "папка пропала\t{path}")?;
    }
    Ok(())
}

fn init(folder: &str, name: &str, kind: &str, icon: Option<&str>, out: &mut Output<'_>) -> Result<(), Failure> {
    let registry = SpaceRegistry::default();
    let name_key = title_key(name);
    if let Some(clash) = registry.load().spaces.into_iter().find(|space| title_key(space.name()) == name_key) {
        return Err(FolioError::SpaceAlreadyRegistered(clash.name().to_owned()).into());
    }
    let kind = SpaceKind::parse(kind).unwrap_or(SpaceKind::Blank);
    let space = create_space(&expand_path(folder), name, icon, kind, Local::now())?;
    registry.register(&space)?;
    writeln!(out, "Создано пространство «{}»: {}", space.name(), space.root_path())?;
    Ok(())
}

fn search(space: Option<&str>, query: &str, limit: usize, out: &mut Output<'_>) -> Result<(), Failure> {
    let workspace = Workspace::load();
    let spaces = match space {
        Some(name) => vec![workspace.space(Some(name))?],
        None => workspace.library().spaces,
    };
    let has_indexable_term = query.split_whitespace().any(|term| term.chars().count() >= MINIMUM_TERM_LENGTH);
    let hits = if has_indexable_term {
        let ids: Vec<_> = spaces.iter().map(|snapshot| snapshot.space.id()).collect();
        workspace.synced_index()?.search(query, Some(&ids), limit)?
    } else {
        let refs: Vec<&SpaceSnapshot> = spaces.iter().map(|snapshot| snapshot.as_ref()).collect();
        index::title_search(query, &refs, limit)
    };
    if hits.is_empty() {
        writeln!(out, "Ничего не найдено")?;
    }
    for hit in hits {
        writeln!(out, "{} — {}", hit.title, workspace.describe(&hit.page))?;
        let snippet = hit
            .snippet
            .replace([HIGHLIGHT_START, HIGHLIGHT_END], "**")
            .split(['\n', '\r'])
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !snippet.is_empty() {
            writeln!(out, "    {snippet}")?;
        }
    }
    Ok(())
}

fn check(space: Option<&str>, all: bool, out: &mut Output<'_>) -> Result<ExitCode, Failure> {
    let workspace = Workspace::load();
    let library = workspace.library();
    let targets = if all { library.spaces.clone() } else { vec![workspace.space(space)?] };
    let mut has_issues = false;
    for snapshot in targets {
        let issues = checker::check(&snapshot, &library);
        let summary = if issues.is_empty() { "проблем нет".to_owned() } else { format!("проблем — {}", issues.len()) };
        writeln!(out, "«{}»: {summary}", snapshot.space.name())?;
        for issue in &issues {
            writeln!(out, "  {}: {}", issue.path, issue.message)?;
        }
        has_issues |= !issues.is_empty();
    }
    Ok(if has_issues { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}

struct TreePrinter<'a> {
    snapshot: &'a SpaceSnapshot,
    shows_paths: bool,
    maximum_depth: Option<usize>,
}

impl TreePrinter<'_> {
    fn render(&self, root: &PageNode, out: &mut Output<'_>) -> io::Result<()> {
        writeln!(out, "{}", self.label(root, false))?;
        self.lines(root, "", 1, out)
    }

    fn lines(&self, parent: &PageNode, prefix: &str, depth: usize, out: &mut Output<'_>) -> io::Result<()> {
        let children: Vec<&PageNode> = self.snapshot.children(parent).collect();
        for (index, node) in children.iter().enumerate() {
            let is_last = index + 1 == children.len();
            let hides_children = self.maximum_depth.is_some_and(|maximum| depth >= maximum);
            writeln!(out, "{prefix}{}{}", if is_last { "└─ " } else { "├─ " }, self.label(node, hides_children))?;
            if !hides_children {
                let next_prefix = format!("{prefix}{}", if is_last { "   " } else { "│  " });
                self.lines(node, &next_prefix, depth + 1, out)?;
            }
        }
        Ok(())
    }

    fn label(&self, node: &PageNode, hides_children: bool) -> String {
        let mut label = node.title.clone();
        if let Some(status) = &node.status {
            label.push_str(&format!("  [{}]", status.label));
        }
        if hides_children && !node.children.is_empty() {
            label.push_str(&format!("  (+{})", node.children.len()));
        }
        if self.shows_paths {
            label.push_str(&format!("  — {}", if node.path.is_empty() { "/" } else { &node.path }));
        }
        label
    }
}

/// `~` — домашняя папка, относительный путь — от текущей папки.
fn expand_path(folder: &str) -> PathBuf {
    let expanded = match folder.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_default();
            home.join(rest.trim_start_matches(['/', '\\']))
        }
        _ => PathBuf::from(folder),
    };
    std::path::absolute(&expanded).unwrap_or(expanded)
}

fn display(path: &Path) -> String {
    folio_core::fsutil::display_path(path)
}
