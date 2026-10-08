//! Команды для интерфейса. Работа с диском и git идёт в пуле блокирующих задач, окно не подвисает.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use chrono::Local;
use folio_core::attachments;
use folio_core::bootstrap::{SpaceKind, create_space as bootstrap_space};
use folio_core::git::GitRepository;
use folio_core::index::SearchHit;
use folio_core::operations::{PageOperations, folder_name};
use folio_core::page_writer;
use folio_core::registry::FolioPaths;
use folio_core::text::{PAGE_FILE_NAME, natural_cmp, page_path};
use folio_core::{PageRef, Space, fsutil};
use serde::Serialize;
use tauri::{AppHandle, State, WebviewUrl, WebviewWindowBuilder};

use crate::Session;
use crate::dto::{self, AttachmentDto, AttachmentReply, HistoryDto, LibraryDto, PageDto, PageEntry, SaveResult, VersionRequest};
use crate::library::Library;
use crate::system;

type Shared<'a> = State<'a, Arc<Library>>;
type Reply<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Reply<T> + Send + 'static) -> Reply<T> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|error| error.to_string())?
}

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn page_not_found(page: &PageRef) -> String {
    format!("Страница «{}» не найдена", if page.path.is_empty() { "главная" } else { &page.path })
}

// MARK: - Дерево, страницы, поиск

#[tauri::command]
pub fn get_library(library: Shared<'_>) -> LibraryDto {
    LibraryDto::new(&library.data())
}

#[tauri::command]
pub fn get_pages_index(library: Shared<'_>) -> Vec<PageEntry> {
    dto::pages_index(&library.library_snapshot())
}

#[tauri::command]
pub async fn get_page(library: Shared<'_>, page: PageRef, version: Option<VersionRequest>) -> Reply<PageDto> {
    let library = library.inner().clone();
    blocking(move || {
        let all = library.library_snapshot();
        let snapshot = all.snapshot(page.space_id).ok_or_else(|| page_not_found(&page))?;
        let node = snapshot.page(&page.path).ok_or_else(|| page_not_found(&page))?;
        let (text, modified) = match &version {
            Some(version) => {
                let text = GitRepository::new(&snapshot.space.root).contents(&version.path, &version.hash).map_err(text)?;
                (text, Some(version.date))
            }
            None => {
                let file = snapshot.file_path(&page.path);
                let bytes = fs::read(&file).map_err(text)?;
                let modified = fs::metadata(&file).ok().map(|metadata| fsutil::modified_seconds(&metadata) as i64);
                (String::from_utf8(bytes).map_err(|_| "файл не в кодировке UTF-8".to_owned())?, modified)
            }
        };
        Ok(dto::page_dto(&page, node, &text, &all, modified, version.as_ref()))
    })
    .await
}

#[tauri::command]
pub async fn search(library: Shared<'_>, query: String) -> Reply<Vec<SearchHit>> {
    let library = library.inner().clone();
    blocking(move || Ok(library.search(&query))).await
}

#[tauri::command]
pub async fn backlinks(library: Shared<'_>, page: PageRef) -> Reply<Vec<SearchHit>> {
    let library = library.inner().clone();
    blocking(move || Ok(library.backlinks(&page))).await
}

#[tauri::command]
pub async fn page_attachments(library: Shared<'_>, page: PageRef) -> Reply<Vec<AttachmentDto>> {
    let library = library.inner().clone();
    blocking(move || {
        let snapshot = library.snapshot(page.space_id).ok_or_else(|| page_not_found(&page))?;
        let folder = snapshot.folder_path(&page.path);
        let mut files: Vec<AttachmentDto> = fs::read_dir(&folder)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let metadata = entry.metadata().ok()?;
                let hidden = fsutil::is_hidden(&name, &metadata)
                    || name.eq_ignore_ascii_case(PAGE_FILE_NAME)
                    || name.eq_ignore_ascii_case("CLAUDE.md")
                    || name.starts_with(fsutil::TEMPORARY_PREFIX);
                (metadata.is_file() && !hidden).then(|| AttachmentDto { path: fsutil::display_path(&entry.path()), size: metadata.len(), name })
            })
            .collect();
        files.sort_by(|lhs, rhs| natural_cmp(&lhs.name, &rhs.name));
        Ok(files)
    })
    .await
}

#[tauri::command]
pub async fn page_history(library: Shared<'_>, page: PageRef) -> Reply<HistoryDto> {
    let library = library.inner().clone();
    blocking(move || {
        let snapshot = library.snapshot(page.space_id).ok_or_else(|| page_not_found(&page))?;
        let repository = GitRepository::new(&snapshot.space.root);
        Ok(HistoryDto {
            revisions: repository.history(&page_path::page_file(&page.path), 200).unwrap_or_default().into_iter().map(Into::into).collect(),
            has_uncommitted_changes: repository.is_initialized() && repository.has_changes().unwrap_or(false),
        })
    })
    .await
}

// MARK: - Операции со страницами

#[tauri::command]
pub async fn templates(library: Shared<'_>, space_id: String) -> Reply<Vec<String>> {
    let library = library.inner().clone();
    blocking(move || {
        let id = uuid::Uuid::parse_str(&space_id).map_err(text)?;
        Ok(library.snapshot(id).map(|snapshot| PageOperations::new(&snapshot).template_names()).unwrap_or_default())
    })
    .await
}

#[tauri::command]
pub async fn create_page(library: Shared<'_>, parent: PageRef, title: String, template: Option<String>) -> Reply<PageRef> {
    let library = library.inner().clone();
    blocking(move || {
        let snapshot = library.snapshot(parent.space_id).ok_or_else(|| page_not_found(&parent))?;
        let path = PageOperations::new(&snapshot).create_page(&title, &parent.path, template.as_deref(), Local::now()).map_err(text)?;
        library.note_own_write(&snapshot.file_path(&path));
        library.rescan(parent.space_id);
        Ok(PageRef::new(parent.space_id, path))
    })
    .await
}

#[tauri::command]
pub async fn rename_page(library: Shared<'_>, page: PageRef, title: String) -> Reply<PageRef> {
    let library = library.inner().clone();
    blocking(move || {
        let all = library.library_snapshot();
        let snapshot = all.snapshot(page.space_id).ok_or_else(|| page_not_found(&page))?.clone();
        let result = PageOperations::new(&snapshot).rename_page(&page.path, &title, &all).map_err(text)?;
        library.note_own_write(&snapshot.file_path(&result.new_path));
        library.rescan(page.space_id);
        Ok(PageRef::new(page.space_id, result.new_path))
    })
    .await
}

#[tauri::command]
pub async fn trash_page(library: Shared<'_>, page: PageRef) -> Reply<()> {
    let library = library.inner().clone();
    blocking(move || {
        let snapshot = library.snapshot(page.space_id).ok_or_else(|| page_not_found(&page))?;
        PageOperations::new(&snapshot).trash_page(&page.path).map_err(text)?;
        library.rescan(page.space_id);
        Ok(())
    })
    .await
}

/// Снимок всех изменений пространства; `false`, если сохранять нечего.
#[tauri::command]
pub async fn commit_space(library: Shared<'_>, space_id: String, message: String) -> Reply<bool> {
    let library = library.inner().clone();
    blocking(move || {
        let id = uuid::Uuid::parse_str(&space_id).map_err(text)?;
        let snapshot = library.snapshot(id).ok_or("Пространство не найдено")?;
        let repository = GitRepository::new(&snapshot.space.root);
        repository.initialize().map_err(text)?;
        let committed = repository.commit_all(&message).map_err(text)?;
        library.emit_history_changed(id);
        Ok(committed)
    })
    .await
}

// MARK: - Правка

/// Пишет новое тело страницы. Если файл на диске изменился с последнего показа — конфликт, ничего не пишем.
#[tauri::command]
pub async fn editor_save(library: Shared<'_>, page_key: String, markdown: String, baseline: Option<String>, force: bool) -> Reply<SaveResult> {
    let library = library.inner().clone();
    blocking(move || {
        let page = dto::parse_page_key(&page_key).ok_or("неизвестная страница")?;
        let snapshot = library.snapshot(page.space_id).ok_or_else(|| page_not_found(&page))?;
        let file = snapshot.file_path(&page.path);
        let disk = fs::read_to_string(&file).map_err(text)?;
        let disk_hash = dto::text_hash(&disk);
        if !force && baseline.as_deref().is_some_and(|baseline| baseline != disk_hash) {
            return Ok(SaveResult::Conflict);
        }
        let composed = page_writer::replacing_body(&disk, &markdown);
        if composed == disk {
            return Ok(SaveResult::Unchanged { hash: disk_hash });
        }
        fsutil::write_atomic(&file, composed.as_bytes()).map_err(text)?;
        library.note_own_write(&file);
        let refresh = library.clone();
        tauri::async_runtime::spawn_blocking(move || refresh.after_own_write(&page));
        Ok(SaveResult::Saved { hash: dto::text_hash(&composed) })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PropertiesResult {
    old_hash: String,
    new_hash: String,
}

#[tauri::command]
pub async fn set_properties(library: Shared<'_>, page: PageRef, status: Option<String>, labels: Vec<String>) -> Reply<PropertiesResult> {
    let library = library.inner().clone();
    blocking(move || {
        let snapshot = library.snapshot(page.space_id).ok_or_else(|| page_not_found(&page))?;
        let file = snapshot.file_path(&page.path);
        let disk = fs::read_to_string(&file).map_err(text)?;
        let updated = page_writer::setting_properties(&disk, status.as_deref(), &labels);
        let result = PropertiesResult { old_hash: dto::text_hash(&disk), new_hash: dto::text_hash(&updated) };
        if updated != disk {
            fsutil::write_atomic(&file, updated.as_bytes()).map_err(text)?;
            library.after_own_write(&page);
        }
        Ok(result)
    })
    .await
}

fn page_folder(library: &Library, page: &PageRef) -> Reply<PathBuf> {
    let snapshot = library.snapshot(page.space_id).ok_or_else(|| page_not_found(page))?;
    snapshot.page(&page.path).ok_or_else(|| page_not_found(page))?;
    Ok(snapshot.folder_path(&page.path))
}

/// Картинка из буфера обмена или перетащенный файл: кладём рядом со страницей под безопасным именем.
#[tauri::command]
pub async fn save_attachment(library: Shared<'_>, page: PageRef, name: String, data: String) -> Reply<AttachmentReply> {
    let library = library.inner().clone();
    blocking(move || {
        let folder = page_folder(&library, &page)?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(data.as_bytes()).map_err(text)?;
        let file_name = attachments::available_name(if name.is_empty() { "Вставка" } else { &name }, &folder, Local::now());
        fsutil::write_atomic(&folder.join(&file_name), &bytes).map_err(text)?;
        Ok(AttachmentReply { is_image: attachments::is_image(&file_name), path: file_name.clone(), name: file_name })
    })
    .await
}

#[tauri::command]
pub async fn copy_attachments(library: Shared<'_>, page: PageRef, paths: Vec<String>) -> Reply<Vec<AttachmentReply>> {
    let library = library.inner().clone();
    blocking(move || {
        let folder = page_folder(&library, &page)?;
        paths
            .iter()
            .map(|source| {
                let source = Path::new(source);
                let original = source.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                let file_name = attachments::available_name(&original, &folder, Local::now());
                fs::copy(source, folder.join(&file_name)).map_err(|error| format!("{original}: {error}"))?;
                Ok(AttachmentReply { is_image: attachments::is_image(&file_name), path: file_name, name: original })
            })
            .collect()
    })
    .await
}

// MARK: - Проводник, ссылки, буфер обмена

#[tauri::command]
pub async fn open_link(app: AppHandle, library: Shared<'_>, href: String, page: PageRef) -> Reply<()> {
    let library = library.inner().clone();
    blocking(move || {
        let snapshot = library.snapshot(page.space_id).ok_or_else(|| page_not_found(&page))?;
        match system::resolve_link(&href, &snapshot, &page.path)? {
            Some(target) => system::open_target(&app, target),
            None => Ok(()),
        }
    })
    .await
}

/// Путь должен лежать внутри одного из пространств: интерфейс не открывает произвольные файлы.
fn inside_space(library: &Library, path: &str) -> Reply<PathBuf> {
    let path = fsutil::normalize(Path::new(path));
    let allowed = library.data().snapshots.iter().any(|snapshot| snapshot.space.contains(&path));
    if allowed { Ok(path) } else { Err("файл вне пространств Folio".to_owned()) }
}

#[tauri::command]
pub async fn open_file(app: AppHandle, library: Shared<'_>, path: String) -> Reply<()> {
    let library = library.inner().clone();
    blocking(move || {
        let path = inside_space(&library, &path)?;
        system::open_target(&app, system::target_for_file(path))
    })
    .await
}

#[tauri::command]
pub async fn reveal_page(app: AppHandle, library: Shared<'_>, page: PageRef) -> Reply<()> {
    let library = library.inner().clone();
    blocking(move || {
        let snapshot = library.snapshot(page.space_id).ok_or_else(|| page_not_found(&page))?;
        system::reveal(&app, &snapshot.file_path(&page.path))
    })
    .await
}

#[tauri::command]
pub async fn reveal_path(app: AppHandle, library: Shared<'_>, path: String) -> Reply<()> {
    let library = library.inner().clone();
    blocking(move || {
        let path = inside_space(&library, &path)?;
        system::reveal(&app, &path)
    })
    .await
}

#[tauri::command]
pub fn copy_page_path(library: Shared<'_>, page: PageRef) -> Reply<()> {
    let snapshot = library.snapshot(page.space_id).ok_or_else(|| page_not_found(&page))?;
    system::copy_text(&fsutil::display_path(&snapshot.file_path(&page.path)))
}

// MARK: - Пространства

#[tauri::command]
pub async fn create_default_spaces(library: Shared<'_>) -> Reply<()> {
    let library = library.inner().clone();
    blocking(move || {
        let mut errors = Vec::new();
        for (name, kind) in [("Работа", SpaceKind::Work), ("Личное", SpaceKind::Personal)] {
            if library.space_name_taken(name) {
                continue;
            }
            let folder = FolioPaths::default_spaces_folder().join(name);
            let space = Space::open(&folder).or_else(|_| bootstrap_space(&folder, name, None, kind, Local::now()));
            if let Err(error) = space.and_then(|space| library.registry.register(&space)) {
                errors.push(error.to_string());
            }
        }
        library.reload();
        if errors.is_empty() { Ok(()) } else { Err(errors.join("\n")) }
    })
    .await
}

#[tauri::command]
pub async fn create_space(library: Shared<'_>, name: String, kind: String, parent_folder: String) -> Reply<()> {
    let library = library.inner().clone();
    blocking(move || {
        let kind = SpaceKind::parse(&kind).unwrap_or(SpaceKind::Blank);
        let root = Path::new(&parent_folder).join(folder_name(&name));
        let space = bootstrap_space(&root, &name, None, kind, Local::now()).map_err(text)?;
        library.registry.register(&space).map_err(text)?;
        library.reload();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn add_space(library: Shared<'_>, folder: String) -> Reply<()> {
    let library = library.inner().clone();
    blocking(move || {
        let space = Space::open(Path::new(&folder)).map_err(text)?;
        library.registry.register(&space).map_err(text)?;
        library.reload();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn remove_space(library: Shared<'_>, space_id: String) -> Reply<()> {
    let library = library.inner().clone();
    blocking(move || {
        let id = uuid::Uuid::parse_str(&space_id).map_err(text)?;
        library.registry.unregister_id(id).map_err(text)?;
        library.forget_space_index(id);
        library.reload();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn forget_missing(library: Shared<'_>, path: String) -> Reply<()> {
    let library = library.inner().clone();
    blocking(move || {
        library.registry.unregister_path(&path).map_err(text)?;
        library.reload();
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn default_spaces_folder() -> String {
    fsutil::display_path(&FolioPaths::default_spaces_folder())
}

// MARK: - Окна и командная строка

#[tauri::command]
pub async fn new_window(app: AppHandle) -> Reply<()> {
    let label = format!("window-{}", uuid::Uuid::new_v4().simple());
    WebviewWindowBuilder::new(&app, label, WebviewUrl::App("index.html".into()))
        .title("Folio")
        .inner_size(1280.0, 820.0)
        .min_inner_size(720.0, 480.0)
        .build()
        .map(|_| ())
        .map_err(text)
}

#[tauri::command]
pub fn take_pending_links(session: State<'_, Session>) -> Vec<String> {
    std::mem::take(&mut *session.pending_links.lock().unwrap())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliStatus {
    folder: Option<String>,
    installed: bool,
}

#[tauri::command]
pub fn cli_status(app: AppHandle) -> CliStatus {
    let folder = system::cli_folder(&app);
    #[cfg(windows)]
    let installed = folder.as_deref().is_some_and(system::path_variable::contains);
    #[cfg(not(windows))]
    let installed = false;
    CliStatus { folder: folder.map(|folder| fsutil::display_path(&folder)), installed }
}

#[tauri::command]
pub fn cli_install(app: AppHandle) -> Reply<()> {
    let folder = system::cli_folder(&app).ok_or("folio.exe не найден рядом с приложением")?;
    #[cfg(windows)]
    return system::path_variable::add(&folder);
    #[cfg(not(windows))]
    {
        let _ = folder;
        Err("только для Windows".to_owned())
    }
}
