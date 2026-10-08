use std::cmp::Ordering;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, SecondsFormat, Utc};
use rayon::prelude::*;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::error::{FolioError, IoContext, Result};
use crate::frontmatter::{PageDocument, PageFrontmatter, PageStatus};
use crate::fsutil;
use crate::links::WikiLink;
use crate::text::{PAGE_FILE_NAME, natural_cmp, page_path, title_key};

/// UUID как его пишет Swift: заглавными буквами. Так манифесты не различаются между macOS и Windows.
pub fn uuid_string(id: Uuid) -> String {
    id.hyphenated().encode_upper(&mut Uuid::encode_buffer()).to_owned()
}

pub(crate) mod uuid_upper {
    use super::*;

    pub fn serialize<S: Serializer>(id: &Uuid, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&uuid_string(*id))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Uuid, D::Error> {
        let text = String::deserialize(deserializer)?;
        Uuid::parse_str(&text).map_err(serde::de::Error::custom)
    }
}

mod iso_date {
    use super::*;

    pub fn serialize<S: Serializer>(date: &DateTime<Utc>, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&date.to_rfc3339_opts(SecondsFormat::Secs, true))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> std::result::Result<DateTime<Utc>, D::Error> {
        let text = String::deserialize(deserializer)?;
        DateTime::parse_from_rfc3339(&text).map(|date| date.with_timezone(&Utc)).map_err(serde::de::Error::custom)
    }
}

/// Манифест `.folio/space.json`. Поля по алфавиту — как пишет `JSONEncoder` с `.sortedKeys`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpaceManifest {
    #[serde(with = "iso_date")]
    pub created: DateTime<Utc>,
    pub icon: String,
    #[serde(with = "uuid_upper")]
    pub id: Uuid,
    pub name: String,
}

impl SpaceManifest {
    pub fn new(name: impl Into<String>, icon: impl Into<String>) -> Self {
        SpaceManifest { created: Utc::now().with_nanosecond_zero(), icon: icon.into(), id: Uuid::new_v4(), name: name.into() }
    }
}

trait NanosecondZero {
    fn with_nanosecond_zero(self) -> Self;
}

impl NanosecondZero for DateTime<Utc> {
    fn with_nanosecond_zero(self) -> Self {
        use chrono::Timelike;
        self.with_nanosecond(0).unwrap_or(self)
    }
}

/// Пространство — папка со страницами, своей историей git и манифестом `.folio/space.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct Space {
    pub manifest: SpaceManifest,
    pub root: PathBuf,
}

impl Space {
    pub const MANIFEST_FOLDER: &'static str = ".folio";
    pub const MANIFEST_FILE: &'static str = "space.json";

    pub fn new(manifest: SpaceManifest, root: &Path) -> Space {
        Space { manifest, root: fsutil::normalize(root) }
    }

    pub fn open(root: &Path) -> Result<Space> {
        let url = root.join(Self::MANIFEST_FOLDER).join(Self::MANIFEST_FILE);
        let data = fs::read(&url).map_err(|_| FolioError::NotASpace(fsutil::display_path(root)))?;
        let manifest: SpaceManifest = serde_json::from_slice(&data)
            .map_err(|error| FolioError::io(url.display(), std::io::Error::new(std::io::ErrorKind::InvalidData, error)))?;
        Ok(Space::new(manifest, root))
    }

    pub fn write_manifest(&self) -> Result<()> {
        let folder = self.root.join(Self::MANIFEST_FOLDER);
        fs::create_dir_all(&folder).at(&folder)?;
        let url = folder.join(Self::MANIFEST_FILE);
        let json = serde_json::to_string_pretty(&self.manifest).expect("манифест сериализуется");
        fsutil::write_atomic(&url, json.as_bytes()).at(&url)
    }

    pub fn id(&self) -> Uuid {
        self.manifest.id
    }

    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    pub fn icon(&self) -> &str {
        &self.manifest.icon
    }

    pub fn root_path(&self) -> String {
        fsutil::display_path(&self.root)
    }

    /// Лежит ли путь внутри пространства.
    pub fn contains(&self, path: &Path) -> bool {
        fsutil::is_within(path, &self.root)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRef {
    #[serde(with = "uuid_upper")]
    pub space_id: Uuid,
    /// Путь папки страницы относительно корня пространства через `/`; пустая строка — главная.
    pub path: String,
}

impl PageRef {
    pub fn new(space_id: Uuid, path: impl Into<String>) -> Self {
        PageRef { space_id, path: path.into() }
    }

    pub fn is_home(&self) -> bool {
        self.path.is_empty()
    }
}

/// Страница в дереве. Дети хранятся путями: дерево плоское, без копий поддеревьев.
#[derive(Debug, Clone, PartialEq)]
pub struct PageNode {
    pub path: String,
    pub title: String,
    pub page_id: Option<String>,
    pub status: Option<PageStatus>,
    pub labels: Vec<String>,
    pub order: Option<f64>,
    /// Пути детей в порядке показа.
    pub children: Vec<String>,
    /// Время изменения `index.md`: по нему индекс поиска понимает, что перечитывать.
    pub modified: f64,
}

impl PageNode {
    pub fn folder_name(&self) -> &str {
        page_path::name(&self.path)
    }

    pub fn parent_path(&self) -> Option<&str> {
        page_path::parent(&self.path)
    }

    pub fn display_order(lhs: &PageNode, rhs: &PageNode) -> Ordering {
        match (lhs.order, rhs.order) {
            (Some(left), Some(right)) if left != right => left.partial_cmp(&right).unwrap_or(Ordering::Equal),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            _ => natural_cmp(&lhs.title, &rhs.title),
        }
    }

    /// Те же ли заголовок, статус, метки и порядок — то есть видно ли изменение в дереве.
    pub fn same_header(&self, other: &PageNode) -> bool {
        self.title == other.title
            && self.page_id == other.page_id
            && self.status == other.status
            && self.labels == other.labels
            && self.order == other.order
    }
}

/// Снимок дерева страниц одного пространства на момент сканирования.
#[derive(Debug, Clone)]
pub struct SpaceSnapshot {
    pub space: Space,
    pub pages: FxHashMap<String, PageNode>,
    title_index: FxHashMap<String, Vec<String>>,
}

impl SpaceSnapshot {
    pub fn new(space: Space, pages: FxHashMap<String, PageNode>) -> Self {
        let mut title_index: FxHashMap<String, Vec<String>> = FxHashMap::default();
        for node in pages.values() {
            title_index.entry(title_key(&node.title)).or_default().push(node.path.clone());
        }
        for paths in title_index.values_mut() {
            paths.sort();
        }
        SpaceSnapshot { space, pages, title_index }
    }

    pub fn home(&self) -> &PageNode {
        &self.pages[""]
    }

    pub fn page(&self, path: &str) -> Option<&PageNode> {
        self.pages.get(path)
    }

    pub fn children(&self, node: &PageNode) -> impl Iterator<Item = &PageNode> {
        node.children.iter().filter_map(|path| self.pages.get(path))
    }

    pub fn pages_titled(&self, title: &str) -> Vec<&PageNode> {
        self.title_index.get(&title_key(title)).map_or_else(Vec::new, |paths| paths.iter().filter_map(|path| self.pages.get(path)).collect())
    }

    pub fn unique_page(&self, title: &str) -> Result<&PageNode> {
        let matches = self.pages_titled(title);
        match matches.as_slice() {
            [] => Err(FolioError::PageNotFound(title.to_owned())),
            [only] => Ok(only),
            _ => Err(FolioError::AmbiguousTitle {
                title: title.to_owned(),
                paths: matches.iter().map(|node| node.path.clone()).collect(),
            }),
        }
    }

    /// Заголовки, которые носят несколько страниц, с путями этих страниц.
    pub fn duplicate_titles(&self) -> Vec<(String, Vec<String>)> {
        let mut result: Vec<(String, Vec<String>)> = self
            .title_index
            .values()
            .filter(|paths| paths.len() > 1)
            .filter_map(|paths| self.pages.get(&paths[0]).map(|node| (node.title.clone(), paths.clone())))
            .collect();
        result.sort();
        result
    }

    /// Цепочка от главной страницы до родителя, без самой страницы.
    pub fn ancestors(&self, path: &str) -> Vec<&PageNode> {
        if path.is_empty() {
            return Vec::new();
        }
        let mut result: Vec<&PageNode> = self.pages.get("").into_iter().collect();
        let mut end = 0;
        while let Some(slash) = path[end..].find('/') {
            end += slash;
            if let Some(node) = self.pages.get(&path[..end]) {
                result.push(node);
            }
            end += 1;
        }
        result
    }

    pub fn folder_path(&self, path: &str) -> PathBuf {
        fsutil::join_segments(&self.space.root, path)
    }

    pub fn file_path(&self, path: &str) -> PathBuf {
        self.folder_path(path).join(PAGE_FILE_NAME)
    }

    pub fn page_ref(&self, path: &str) -> PageRef {
        PageRef::new(self.space.id(), path)
    }

    /// Снимок с обновлённой шапкой одной страницы; `None`, если пересобрать нужно всё дерево.
    pub fn with_updated_page(&self, fresh: PageNode) -> Option<SpaceSnapshot> {
        let current = self.pages.get(&fresh.path)?;
        let title_changed = current.title != fresh.title;
        let order_changed = current.order != fresh.order;
        let mut pages = self.pages.clone();
        let path = fresh.path.clone();
        pages.insert(path.clone(), PageNode { children: current.children.clone(), ..fresh });
        if title_changed || order_changed {
            if let Some(parent) = page_path::parent(&path).map(str::to_owned) {
                let mut siblings = pages.get(&parent)?.children.clone();
                siblings.sort_by(|lhs, rhs| PageNode::display_order(&pages[lhs], &pages[rhs]));
                pages.get_mut(&parent)?.children = siblings;
            }
        }
        if title_changed {
            return Some(SpaceSnapshot::new(self.space.clone(), pages));
        }
        Some(SpaceSnapshot { space: self.space.clone(), pages, title_index: self.title_index.clone() })
    }
}

pub struct SpaceScanner;

impl SpaceScanner {
    const HEADER_BYTE_LIMIT: usize = 4096;

    /// Обходит папки параллельно: одна выдача содержимого папки на страницу и одно чтение начала `index.md`.
    pub fn scan(space: &Space) -> SpaceSnapshot {
        let listing = read_listing(&space.root).unwrap_or_default();
        let nodes = scan_page(&space.root, String::new(), listing, space.name());
        let pages: FxHashMap<String, PageNode> = nodes.into_iter().map(|node| (node.path.clone(), node)).collect();
        SpaceSnapshot::new(space.clone(), pages)
    }

    /// Папка — страница, если в ней лежит `index.md`. Папки на `_` и `.` служебные.
    pub fn is_page_folder(url: &Path) -> bool {
        let Some(name) = url.file_name().and_then(|name| name.to_str()) else { return false };
        if name.starts_with(['_', '.']) || !url.is_dir() {
            return false;
        }
        url.join(PAGE_FILE_NAME).is_file()
    }

    /// Шапка одной страницы без обхода дерева: для правок, после которых дерево не меняется.
    pub fn read_page(snapshot: &SpaceSnapshot, path: &str) -> Option<PageNode> {
        let file = snapshot.file_path(path);
        let metadata = fs::metadata(&file).ok()?;
        let frontmatter = read_frontmatter(&file);
        Some(make_node(path.to_owned(), frontmatter, fsutil::modified_seconds(&metadata), snapshot.space.name(), Vec::new()))
    }

    /// Читает только начало файла: расшифровки бывают большими, а для дерева нужна одна шапка.
    pub fn read_frontmatter(file: &Path) -> PageFrontmatter {
        read_frontmatter(file)
    }
}

#[derive(Default)]
struct Listing {
    index: Option<(String, f64)>,
    folders: Vec<String>,
}

fn read_listing(folder: &Path) -> Option<Listing> {
    let mut listing = Listing::default();
    for entry in fs::read_dir(folder).ok()?.flatten() {
        let Ok(name) = entry.file_name().into_string() else { continue };
        let Ok(metadata) = entry.metadata() else { continue };
        if metadata.is_dir() {
            if !name.starts_with(['_', '.']) && !fsutil::is_hidden(&name, &metadata) {
                listing.folders.push(name);
            }
        } else if metadata.is_file() && name.eq_ignore_ascii_case(PAGE_FILE_NAME) {
            listing.index = Some((name, fsutil::modified_seconds(&metadata)));
        }
    }
    Some(listing)
}

/// Страница и все её потомки; первая в списке — сама страница.
fn scan_page(folder: &Path, path: String, listing: Listing, space_name: &str) -> Vec<PageNode> {
    let (frontmatter, modified) = match &listing.index {
        Some((name, modified)) => (read_frontmatter(&folder.join(name)), *modified),
        None => (PageFrontmatter::default(), 0.0),
    };

    let subtrees: Vec<Vec<PageNode>> = listing
        .folders
        .into_par_iter()
        .filter_map(|name| {
            let child_folder = folder.join(&name);
            let child_listing = read_listing(&child_folder)?;
            child_listing.index.as_ref()?;
            Some(scan_page(&child_folder, page_path::join(&path, &name), child_listing, space_name))
        })
        .collect();

    let mut child_heads: Vec<&PageNode> = subtrees.iter().map(|subtree| &subtree[0]).collect();
    child_heads.sort_by(|lhs, rhs| PageNode::display_order(lhs, rhs));
    let children = child_heads.iter().map(|node| node.path.clone()).collect();

    let node = make_node(path, frontmatter, modified, space_name, children);
    let mut result = Vec::with_capacity(1 + subtrees.iter().map(Vec::len).sum::<usize>());
    result.push(node);
    result.extend(subtrees.into_iter().flatten());
    result
}

fn make_node(path: String, frontmatter: PageFrontmatter, modified: f64, space_name: &str, children: Vec<String>) -> PageNode {
    let fallback = if path.is_empty() { space_name.to_owned() } else { page_path::name(&path).to_owned() };
    PageNode {
        title: frontmatter.title.unwrap_or(fallback),
        page_id: frontmatter.id,
        status: frontmatter.status,
        labels: frontmatter.labels,
        order: frontmatter.order,
        children,
        modified,
        path,
    }
}

fn read_frontmatter(file: &Path) -> PageFrontmatter {
    let Ok(mut handle) = File::open(file) else { return PageFrontmatter::default() };
    let mut head = vec![0u8; SpaceScanner::HEADER_BYTE_LIMIT];
    let mut filled = 0;
    while filled < head.len() {
        match handle.read(&mut head[filled..]) {
            Ok(0) => break,
            Ok(count) => filled += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    head.truncate(filled);
    if let Some(frontmatter) = PageDocument::parse_frontmatter(&String::from_utf8_lossy(&head)) {
        return frontmatter;
    }
    let content = head.strip_prefix("\u{FEFF}".as_bytes()).unwrap_or(&head);
    let is_truncated_header = filled == SpaceScanner::HEADER_BYTE_LIMIT && content.starts_with(b"---");
    if !is_truncated_header {
        return PageFrontmatter::default();
    }
    let _ = handle.read_to_end(&mut head);
    PageDocument::parse_frontmatter(&String::from_utf8_lossy(&head)).unwrap_or_default()
}

/// Все подключённые пространства сразу — нужно для ссылок между пространствами.
#[derive(Debug, Clone, Default)]
pub struct LibrarySnapshot {
    pub spaces: Vec<Arc<SpaceSnapshot>>,
}

impl LibrarySnapshot {
    pub fn new(spaces: Vec<Arc<SpaceSnapshot>>) -> Self {
        LibrarySnapshot { spaces }
    }

    /// Сканирует пространства параллельно.
    pub fn scan(spaces: &[Space]) -> Self {
        LibrarySnapshot { spaces: spaces.par_iter().map(|space| Arc::new(SpaceScanner::scan(space))).collect() }
    }

    pub fn snapshot(&self, id: Uuid) -> Option<&Arc<SpaceSnapshot>> {
        self.spaces.iter().find(|snapshot| snapshot.space.id() == id)
    }

    pub fn snapshot_named(&self, name: &str) -> Option<&Arc<SpaceSnapshot>> {
        let key = title_key(name);
        self.spaces.iter().find(|snapshot| title_key(snapshot.space.name()) == key)
    }

    pub fn node(&self, page: &PageRef) -> Option<&PageNode> {
        self.snapshot(page.space_id)?.page(&page.path)
    }

    pub fn resolve(&self, link: &WikiLink, from_space: Uuid) -> Option<PageRef> {
        let target = match &link.space {
            Some(name) => self.snapshot_named(name),
            None => self.snapshot(from_space),
        }?;
        target.pages_titled(&link.title).first().map(|node| target.page_ref(&node.path))
    }
}
