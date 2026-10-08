//! Данные, которые уходят в интерфейс.

use std::hash::Hasher;

use chrono::{Local, TimeZone};
use folio_core::git::GitRevision;
use folio_core::space::uuid_string;
use folio_core::text::russian_long_date_time;
use folio_core::{LibrarySnapshot, PageDocument, PageNode, PageRef, PageStatus, SpaceSnapshot, WikiLinkParser};
use rustc_hash::{FxHashMap, FxHasher};
use serde::{Deserialize, Serialize};

use crate::library::LibraryData;
use crate::protocol;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusDto {
    pub label: String,
    pub color: &'static str,
}

impl From<&PageStatus> for StatusDto {
    fn from(status: &PageStatus) -> Self {
        StatusDto { label: status.label.clone(), color: status.color.as_str() }
    }
}

/// Узел дерева. Узлы идут в прямом порядке обхода с глубиной — интерфейсу так проще показывать
/// только раскрытые ветки, не собирая дерево заново.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDto {
    pub path: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<StatusDto>,
    pub depth: u32,
    pub child_count: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceDto {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub root_path: String,
    pub nodes: Vec<NodeDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDto {
    pub loaded: bool,
    pub revision: u64,
    pub spaces: Vec<SpaceDto>,
    pub missing_paths: Vec<String>,
}

impl LibraryDto {
    pub fn new(data: &LibraryData) -> Self {
        LibraryDto {
            loaded: data.loaded,
            revision: data.revision,
            spaces: data.snapshots.iter().map(|snapshot| space_dto(snapshot)).collect(),
            missing_paths: data.missing.clone(),
        }
    }
}

fn space_dto(snapshot: &SpaceSnapshot) -> SpaceDto {
    let mut nodes = Vec::with_capacity(snapshot.pages.len());
    let mut stack: Vec<(&PageNode, u32)> = vec![(snapshot.home(), 0)];
    while let Some((node, depth)) = stack.pop() {
        nodes.push(NodeDto {
            path: node.path.clone(),
            title: node.title.clone(),
            page_id: node.page_id.clone(),
            status: node.status.as_ref().map(StatusDto::from),
            depth,
            child_count: node.children.len() as u32,
        });
        let children: Vec<&PageNode> = snapshot.children(node).collect();
        stack.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
    }
    SpaceDto {
        id: uuid_string(snapshot.space.id()),
        name: snapshot.space.name().to_owned(),
        icon: snapshot.space.icon().to_owned(),
        root_path: snapshot.space.root_path(),
        nodes,
    }
}

#[derive(Serialize)]
pub struct PageEntry {
    pub space: String,
    pub title: String,
}

/// Список всех страниц для подсказки по `[[`. Интерфейс берёт его один раз на версию дерева.
pub fn pages_index(library: &LibrarySnapshot) -> Vec<PageEntry> {
    library
        .spaces
        .iter()
        .flat_map(|snapshot| snapshot.pages.values().map(|node| PageEntry { space: snapshot.space.name().to_owned(), title: node.title.clone() }))
        .collect()
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VersionRequest {
    pub hash: String,
    pub path: String,
    pub date: i64,
    pub message: String,
}

#[derive(Serialize)]
pub struct ChildDto {
    pub title: String,
}

/// То, что уходит в `window.folio.render` веб-части, без списка страниц и флагов показа — их добавляет интерфейс.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageDto {
    pub page_key: String,
    pub markdown: String,
    pub title: String,
    pub status: Option<StatusDto>,
    pub labels: Vec<String>,
    pub meta: String,
    pub asset_base: String,
    pub links: FxHashMap<String, bool>,
    pub children: Vec<ChildDto>,
    pub space_name: String,
    pub banner: Option<String>,
    /// Отпечаток текста файла: по нему интерфейс узнаёт своё же эхо во время правки.
    pub text_hash: String,
    pub page_id: Option<String>,
}

pub fn page_key(page: &PageRef) -> String {
    format!("{}\n{}", uuid_string(page.space_id), page.path)
}

pub fn parse_page_key(key: &str) -> Option<PageRef> {
    let (space, path) = key.split_once('\n')?;
    Some(PageRef::new(uuid::Uuid::parse_str(space).ok()?, path))
}

pub fn text_hash(text: &str) -> String {
    let mut hasher = FxHasher::default();
    hasher.write(text.as_bytes());
    hasher.write_usize(text.len());
    format!("{:016x}", hasher.finish())
}

pub fn page_dto(
    page: &PageRef,
    node: &PageNode,
    text: &str,
    library: &LibrarySnapshot,
    modified: Option<i64>,
    version: Option<&VersionRequest>,
) -> PageDto {
    let document = PageDocument::parse(text);
    let mut links = FxHashMap::default();
    for link in WikiLinkParser::links(&document.body) {
        let resolved = library.resolve(&link, page.space_id).is_some();
        links.insert(link.key(), resolved);
    }
    let snapshot = library.snapshot(page.space_id);
    let (status, labels, title) = match version {
        None => (node.status.as_ref().map(StatusDto::from), node.labels.clone(), node.title.clone()),
        Some(_) => (
            document.frontmatter.status.as_ref().map(StatusDto::from),
            document.frontmatter.labels.clone(),
            document.frontmatter.title.clone().unwrap_or_else(|| node.title.clone()),
        ),
    };
    let long_date = |seconds: i64| Local.timestamp_opt(seconds, 0).single().map(russian_long_date_time).unwrap_or_default();
    PageDto {
        page_key: page_key(page),
        title,
        status,
        labels,
        meta: modified.map(|seconds| format!("Изменено {}", long_date(seconds))).unwrap_or_default(),
        asset_base: protocol::asset_base(page),
        links,
        children: snapshot.map(|snapshot| snapshot.children(node).map(|child| ChildDto { title: child.title.clone() }).collect()).unwrap_or_default(),
        space_name: snapshot.map(|snapshot| snapshot.space.name().to_owned()).unwrap_or_default(),
        banner: version.map(|version| format!("Старая версия от {} — «{}». Только для просмотра.", long_date(version.date), version.message)),
        text_hash: text_hash(text),
        page_id: node.page_id.clone(),
        markdown: document.body,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionDto {
    pub hash: String,
    pub path: String,
    pub date: i64,
    pub message: String,
    pub author: String,
}

impl From<GitRevision> for RevisionDto {
    fn from(revision: GitRevision) -> Self {
        RevisionDto { date: revision.date.timestamp(), hash: revision.hash, path: revision.path, message: revision.message, author: revision.author }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryDto {
    pub revisions: Vec<RevisionDto>,
    pub has_uncommitted_changes: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentDto {
    pub name: String,
    pub path: String,
    pub size: u64,
}

/// Файл, положенный рядом со страницей; веб-часть вставляет его картинкой или ссылкой.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentReply {
    pub path: String,
    pub name: String,
    pub is_image: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum SaveResult {
    Saved { hash: String },
    Unchanged { hash: String },
    Conflict,
}
