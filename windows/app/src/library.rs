//! Все подключённые пространства: дерево страниц, наблюдение за файлами, индекс поиска.
//!
//! Главное отличие от версии для macOS — обновления точечные. Правка одной страницы
//! перечитывает одну шапку и переиндексирует один файл, а не обходит всё пространство.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock, mpsc};

use folio_core::index::{self, SearchHit, SearchIndex};
use folio_core::registry::{FolioPaths, SpaceRegistry};
use folio_core::space::uuid_string;
use folio_core::text::{PAGE_FILE_NAME, title_key};
use folio_core::{LibrarySnapshot, PageRef, SpaceScanner, SpaceSnapshot, fsutil};
use rustc_hash::{FxHashMap, FxHashSet};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::watcher;

#[derive(Default, Clone)]
pub struct LibraryData {
    pub loaded: bool,
    pub snapshots: Vec<Arc<SpaceSnapshot>>,
    pub missing: Vec<String>,
    /// Растёт, когда меняется дерево, заголовки или статусы.
    pub revision: u64,
}

pub struct Library {
    app: AppHandle,
    pub registry: SpaceRegistry,
    data: RwLock<LibraryData>,
    /// Все изменения снимков идут по очереди; чтение не ждёт.
    updates: Mutex<()>,
    /// Два соединения: поиск не ждёт, пока индекс обновляется после большой правки.
    writer: Mutex<Option<SearchIndex>>,
    reader: Mutex<Option<SearchIndex>>,
    watchers: Mutex<FxHashMap<Uuid, watcher::SpaceWatcher>>,
    /// Время изменения файлов, которые записало само приложение: их эхо от наблюдателя не считается чужой правкой.
    own_writes: Mutex<FxHashMap<PathBuf, f64>>,
    events: mpsc::Sender<watcher::Change>,
}

const SEARCH_LIMIT: usize = 60;

impl Library {
    pub fn start(app: AppHandle) -> Arc<Library> {
        let (sender, receiver) = mpsc::channel();
        let index_path = FolioPaths::index_path();
        let library = Arc::new(Library {
            app,
            registry: SpaceRegistry::default(),
            data: RwLock::new(LibraryData::default()),
            updates: Mutex::new(()),
            writer: Mutex::new(SearchIndex::open(&index_path).map_err(|error| eprintln!("индекс не открылся: {error}")).ok()),
            reader: Mutex::new(SearchIndex::open(&index_path).ok()),
            watchers: Mutex::new(FxHashMap::default()),
            own_writes: Mutex::new(FxHashMap::default()),
            events: sender,
        });
        watcher::spawn_debouncer(Arc::downgrade(&library), receiver);
        let starting = library.clone();
        std::thread::spawn(move || starting.reload());
        library
    }

    pub fn data(&self) -> LibraryData {
        self.data.read().unwrap().clone()
    }

    pub fn snapshot(&self, id: Uuid) -> Option<Arc<SpaceSnapshot>> {
        self.data.read().unwrap().snapshots.iter().find(|snapshot| snapshot.space.id() == id).cloned()
    }

    pub fn library_snapshot(&self) -> LibrarySnapshot {
        LibrarySnapshot::new(self.data.read().unwrap().snapshots.clone())
    }

    /// Перечитывает реестр и все пространства; дальше индекс сверяется в фоне.
    pub fn reload(&self) {
        let _guard = self.updates.lock().unwrap();
        let loaded = self.registry.load();
        let library = LibrarySnapshot::scan(&loaded.spaces);
        {
            let mut data = self.data.write().unwrap();
            data.loaded = true;
            data.snapshots = library.spaces.clone();
            data.missing = loaded.missing_paths;
            data.revision += 1;
        }
        self.restart_watchers();
        self.emit_library_changed();
        for snapshot in &library.spaces {
            self.sync_index(snapshot, None);
        }
        let _ = self.app.emit("index-changed", ());
    }

    fn restart_watchers(&self) {
        let snapshots = self.data.read().unwrap().snapshots.clone();
        let ids: FxHashSet<Uuid> = snapshots.iter().map(|snapshot| snapshot.space.id()).collect();
        let mut watchers = self.watchers.lock().unwrap();
        watchers.retain(|id, _| ids.contains(id));
        for snapshot in snapshots {
            let id = snapshot.space.id();
            if !watchers.contains_key(&id) {
                if let Some(watcher) = watcher::SpaceWatcher::start(id, &snapshot.space.root, self.events.clone()) {
                    watchers.insert(id, watcher);
                }
            }
        }
    }

    /// Полное пересканирование одного пространства — после создания, переименования и удаления страниц.
    pub fn rescan(&self, id: Uuid) {
        let _guard = self.updates.lock().unwrap();
        self.rescan_locked(id);
    }

    fn rescan_locked(&self, id: Uuid) {
        let Some(previous) = self.snapshot(id) else { return };
        let fresh = Arc::new(SpaceScanner::scan(&previous.space));
        let external = self.changed_pages(&previous, &fresh);
        let tree_changed = !same_tree(&previous, &fresh);
        self.replace(fresh.clone(), tree_changed);
        if !external.is_empty() {
            let _ = self.app.emit("pages-changed", &external);
        }
        self.sync_index(&fresh, None);
        let _ = self.app.emit("index-changed", ());
    }

    /// Страницы, чей файл изменился не руками приложения.
    fn changed_pages(&self, previous: &SpaceSnapshot, fresh: &SpaceSnapshot) -> Vec<PageRef> {
        let own = self.own_writes.lock().unwrap();
        fresh
            .pages
            .values()
            .filter(|node| previous.page(&node.path).is_some_and(|old| old.modified != node.modified))
            .filter(|node| own.get(&fresh.file_path(&node.path)) != Some(&node.modified))
            .map(|node| fresh.page_ref(&node.path))
            .collect()
    }

    fn replace(&self, fresh: Arc<SpaceSnapshot>, tree_changed: bool) {
        {
            let mut data = self.data.write().unwrap();
            let id = fresh.space.id();
            if let Some(slot) = data.snapshots.iter_mut().find(|snapshot| snapshot.space.id() == id) {
                *slot = fresh;
            }
            if tree_changed {
                data.revision += 1;
            }
        }
        if tree_changed {
            self.emit_library_changed();
        }
    }

    fn emit_library_changed(&self) {
        let revision = self.data.read().unwrap().revision;
        let _ = self.app.emit("library-changed", revision);
    }

    /// Пачка путей от наблюдателя за папкой пространства.
    pub fn handle_paths(&self, id: Uuid, paths: Vec<PathBuf>) {
        let _guard = self.updates.lock().unwrap();
        let Some(snapshot) = self.snapshot(id) else { return };
        let mut content_pages: Vec<String> = Vec::new();
        let mut folders: FxHashSet<String> = FxHashSet::default();
        let mut structural = false;

        for path in &paths {
            match classify(&snapshot, path) {
                Kind::PageFile(page) => content_pages.push(page),
                Kind::Attachment(page) => {
                    folders.insert(page);
                }
                Kind::Structure => structural = true,
                Kind::Ignored => {}
            }
        }

        if structural {
            self.rescan_locked(id);
        } else if !content_pages.is_empty() {
            content_pages.sort();
            content_pages.dedup();
            self.refresh_pages(&snapshot, &content_pages);
        }
        if !folders.is_empty() {
            let refs: Vec<PageRef> = folders.into_iter().map(|path| snapshot.page_ref(&path)).collect();
            let _ = self.app.emit("folders-changed", &refs);
        }
    }

    /// Перечитывает шапки перечисленных страниц. Дерево пересобирается, только если поменялся заголовок или порядок.
    fn refresh_pages(&self, snapshot: &Arc<SpaceSnapshot>, paths: &[String]) {
        let mut current: SpaceSnapshot = (**snapshot).clone();
        let mut external = Vec::new();
        let mut reindex = Vec::new();
        let mut tree_changed = false;
        for path in paths {
            let Some(fresh) = SpaceScanner::read_page(&current, path) else {
                self.rescan_locked(snapshot.space.id());
                return;
            };
            let file = current.file_path(path);
            let is_own = self.own_writes.lock().unwrap().get(&file) == Some(&fresh.modified);
            let old = &current.pages[path];
            if old.modified == fresh.modified && old.same_header(&fresh) {
                continue;
            }
            if !is_own {
                external.push(current.page_ref(path));
            }
            tree_changed |= !old.same_header(&fresh);
            reindex.push(path.clone());
            match current.with_updated_page(fresh) {
                Some(updated) => current = updated,
                None => {
                    self.rescan_locked(snapshot.space.id());
                    return;
                }
            }
        }
        if reindex.is_empty() {
            return;
        }
        let current = Arc::new(current);
        self.replace(current.clone(), tree_changed);
        if !external.is_empty() {
            let _ = self.app.emit("pages-changed", &external);
        }
        self.sync_index(&current, Some(&reindex));
        let _ = self.app.emit("index-changed", ());
    }

    /// Приложение само записало файл страницы: обновляем снимок и индекс сразу, не дожидаясь наблюдателя.
    pub fn after_own_write(&self, page: &PageRef) {
        let Some(snapshot) = self.snapshot(page.space_id) else { return };
        self.note_own_write(&snapshot.file_path(&page.path));
        let _guard = self.updates.lock().unwrap();
        if let Some(snapshot) = self.snapshot(page.space_id) {
            self.refresh_pages(&snapshot, std::slice::from_ref(&page.path));
        }
    }

    pub fn note_own_write(&self, file: &Path) {
        if let Ok(metadata) = std::fs::metadata(file) {
            self.own_writes.lock().unwrap().insert(file.to_path_buf(), fsutil::modified_seconds(&metadata));
        }
    }

    fn sync_index(&self, snapshot: &SpaceSnapshot, only: Option<&[String]>) {
        let mut writer = self.writer.lock().unwrap();
        let Some(index) = writer.as_mut() else { return };
        let result = match only {
            Some(paths) => index.sync_pages(snapshot, paths),
            None => index.sync(snapshot),
        };
        if let Err(error) = result {
            eprintln!("индекс не обновился: {error}");
        }
    }

    pub fn search(&self, query: &str) -> Vec<SearchHit> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        let data = self.data();
        let snapshots: Vec<&SpaceSnapshot> = data.snapshots.iter().map(|snapshot| snapshot.as_ref()).collect();
        let has_indexable = trimmed.split_whitespace().any(|term| term.chars().count() >= index::MINIMUM_TERM_LENGTH);
        if has_indexable {
            if let Some(reader) = self.reader.lock().unwrap().as_ref() {
                match reader.search(trimmed, None, SEARCH_LIMIT) {
                    Ok(hits) => return hits,
                    Err(error) => eprintln!("поиск не удался: {error}"),
                }
            }
        }
        index::title_search(trimmed, &snapshots, SEARCH_LIMIT)
    }

    pub fn backlinks(&self, page: &PageRef) -> Vec<SearchHit> {
        let Some(snapshot) = self.snapshot(page.space_id) else { return Vec::new() };
        let Some(node) = snapshot.page(&page.path) else { return Vec::new() };
        let reader = self.reader.lock().unwrap();
        let hits = reader.as_ref().and_then(|reader| reader.backlinks(&node.title, &snapshot.space).ok()).unwrap_or_default();
        hits.into_iter().filter(|hit| &hit.page != page).collect()
    }

    pub fn forget_space_index(&self, id: Uuid) {
        if let Some(index) = self.writer.lock().unwrap().as_mut() {
            let _ = index.remove_space(id);
        }
    }

    pub fn space_name_taken(&self, name: &str) -> bool {
        let key = title_key(name);
        self.data.read().unwrap().snapshots.iter().any(|snapshot| title_key(snapshot.space.name()) == key)
    }

    pub fn emit_history_changed(&self, id: Uuid) {
        let _ = self.app.emit("history-changed", uuid_string(id));
    }
}

enum Kind {
    PageFile(String),
    Attachment(String),
    Structure,
    Ignored,
}

/// Что значит изменение пути для дерева: правка страницы, вложение рядом с ней или перестройка дерева.
fn classify(snapshot: &SpaceSnapshot, path: &Path) -> Kind {
    let Ok(relative) = path.strip_prefix(&snapshot.space.root) else { return Kind::Structure };
    let components: Vec<String> = relative.components().map(|component| component.as_os_str().to_string_lossy().into_owned()).collect();
    let Some(name) = components.last() else { return Kind::Structure };
    let parent = components[..components.len() - 1].join("/");

    if components.first().is_some_and(|first| first == ".folio") {
        return Kind::Structure;
    }
    if components.iter().any(|component| component.starts_with(['.', '_'])) || fsutil::is_system_file(name) {
        return Kind::Ignored;
    }
    if name.eq_ignore_ascii_case(PAGE_FILE_NAME) {
        let is_known = snapshot.pages.contains_key(&parent) && path.is_file();
        return if is_known { Kind::PageFile(parent) } else { Kind::Structure };
    }
    let full = components.join("/");
    if snapshot.pages.contains_key(&full) {
        return Kind::Structure;
    }
    if snapshot.pages.contains_key(&parent) {
        // Новая папка рядом со страницей может стать подстраницей; файл — это вложение.
        let looks_like_folder = path.is_dir() || (!path.exists() && Path::new(name).extension().is_none());
        return if looks_like_folder { Kind::Structure } else { Kind::Attachment(parent) };
    }
    Kind::Ignored
}

/// Совпадают ли деревья: те же страницы, заголовки, статусы и порядок детей.
fn same_tree(lhs: &SpaceSnapshot, rhs: &SpaceSnapshot) -> bool {
    lhs.pages.len() == rhs.pages.len()
        && lhs.space == rhs.space
        && lhs.pages.iter().all(|(path, node)| rhs.page(path).is_some_and(|other| other.same_header(node) && other.children == node.children))
}
