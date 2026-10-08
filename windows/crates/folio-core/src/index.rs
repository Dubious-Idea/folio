//! Полнотекстовый индекс всех пространств и таблица ссылок для обратных ссылок.
//! Индекс — кэш: его можно удалить, он пересоберётся из файлов.

use std::fs;
use std::path::Path;
use std::time::Duration;

use rayon::prelude::*;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Serialize;
use uuid::Uuid;

use crate::error::{IoContext, Result};
use crate::frontmatter::PageDocument;
use crate::links::WikiLinkParser;
use crate::space::{PageNode, PageRef, Space, SpaceSnapshot, uuid_string};
use crate::text::{natural_cmp, title_key};

pub const HIGHLIGHT_START: char = '\u{1}';
pub const HIGHLIGHT_END: char = '\u{2}';
/// Поиск по триграммам не находит слова короче трёх букв.
pub const MINIMUM_TERM_LENGTH: usize = 3;
const SCHEMA_VERSION: i32 = 1;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchHit {
    #[serde(rename = "ref")]
    pub page: PageRef,
    pub title: String,
    /// Фрагмент текста; совпадения обрамлены `HIGHLIGHT_START` и `HIGHLIGHT_END`.
    pub snippet: String,
}

pub struct SearchIndex {
    connection: Connection,
}

struct Pending<'a> {
    path: &'a str,
    node: &'a PageNode,
    existing_id: Option<i64>,
}

impl SearchIndex {
    pub fn open(path: &Path) -> Result<SearchIndex> {
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder).at(folder)?;
        }
        let connection = Connection::open(path)?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        migrate(&connection)?;
        Ok(SearchIndex { connection })
    }

    /// Переиндексирует изменившиеся страницы пространства; возвращает число затронутых страниц.
    /// Время изменения берётся из снимка — сканер получил его вместе с содержимым папки.
    pub fn sync(&mut self, snapshot: &SpaceSnapshot) -> Result<usize> {
        self.sync_with(snapshot, None)
    }

    /// То же для перечисленных страниц: после правки одной страницы не нужно сверять всё пространство.
    pub fn sync_pages(&mut self, snapshot: &SpaceSnapshot, paths: &[String]) -> Result<usize> {
        self.sync_with(snapshot, Some(paths))
    }

    fn sync_with(&mut self, snapshot: &SpaceSnapshot, only: Option<&[String]>) -> Result<usize> {
        let space_key = uuid_string(snapshot.space.id());
        // Сверка с базой внутри транзакции: приложение и утилита folio могут обновлять индекс одновременно.
        let transaction = Transaction::new(&mut self.connection, TransactionBehavior::Immediate)?;
        let known = known_documents(&transaction, &space_key, only)?;

        let candidates: Vec<&String> = match only {
            Some(paths) => paths.iter().filter(|path| snapshot.pages.contains_key(*path)).collect(),
            None => snapshot.pages.keys().collect(),
        };
        let changes: Vec<Pending<'_>> = candidates
            .into_iter()
            .filter_map(|path| {
                let node = &snapshot.pages[path];
                let known = known.get(path.as_str());
                if known.is_some_and(|(_, modified)| *modified == node.modified) {
                    return None;
                }
                Some(Pending { path, node, existing_id: known.map(|(id, _)| *id) })
            })
            .collect();
        let removed: Vec<i64> = known.iter().filter(|(path, _)| !snapshot.pages.contains_key(*path)).map(|(_, (id, _))| *id).collect();

        // Файлы читаются и разбираются параллельно, в базу пишутся по очереди.
        let bodies: Vec<(String, Vec<(Option<String>, String)>)> = changes
            .par_iter()
            .map(|change| {
                let text = fs::read_to_string(snapshot.file_path(change.path)).unwrap_or_default();
                let body = PageDocument::parse(&text).body;
                let mut seen = FxHashSet::default();
                let targets = WikiLinkParser::links(&body)
                    .into_iter()
                    .map(|link| (link.space.as_deref().map(title_key), title_key(&link.title)))
                    .filter(|target| seen.insert(target.clone()))
                    .collect();
                (body, targets)
            })
            .collect();

        for (change, (body, targets)) in changes.iter().zip(&bodies) {
            upsert(&transaction, &space_key, change, body, targets)?;
        }
        for id in &removed {
            delete_document(&transaction, *id)?;
        }
        transaction.commit()?;
        Ok(changes.len() + removed.len())
    }

    pub fn search(&self, query: &str, space_ids: Option<&[Uuid]>, limit: usize) -> Result<Vec<SearchHit>> {
        let terms: Vec<&str> = query.split_whitespace().filter(|term| term.chars().count() >= MINIMUM_TERM_LENGTH).collect();
        if terms.is_empty() || space_ids.is_some_and(<[Uuid]>::is_empty) {
            return Ok(Vec::new());
        }
        let expression = terms.iter().map(|term| format!("\"{}\"", term.replace('"', "\"\""))).collect::<Vec<_>>().join(" ");
        let space_filter = space_ids.map_or_else(String::new, |ids| format!(" AND d.space IN ({})", vec!["?"; ids.len()].join(", ")));
        let sql = format!(
            "SELECT d.space, d.path, d.title, snippet(documents_fts, 2, char(1), char(2), '…', 16) \
             FROM documents_fts JOIN documents d ON d.id = documents_fts.rowid \
             WHERE documents_fts MATCH ?{space_filter} \
             ORDER BY bm25(documents_fts, 10.0, 4.0, 1.0) LIMIT ?"
        );
        let mut values: Vec<rusqlite::types::Value> = vec![expression.into()];
        values.extend(space_ids.unwrap_or_default().iter().map(|id| uuid_string(*id).into()));
        values.push((limit as i64).into());
        let mut statement = self.connection.prepare_cached(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(values), hit)?;
        Ok(rows.filter_map(|row| row.ok().flatten()).collect())
    }

    pub fn backlinks(&self, title: &str, space: &Space) -> Result<Vec<SearchHit>> {
        let mut statement = self.connection.prepare_cached(
            "SELECT DISTINCT d.space, d.path, d.title, '' \
             FROM links l JOIN documents d ON d.id = l.document_id \
             WHERE l.target_title = ? AND ((l.target_space IS NULL AND d.space = ?) OR l.target_space = ?) \
             ORDER BY d.title",
        )?;
        let rows = statement.query_map(params![title_key(title), uuid_string(space.id()), title_key(space.name())], hit)?;
        Ok(rows.filter_map(|row| row.ok().flatten()).collect())
    }

    pub fn remove_space(&mut self, id: Uuid) -> Result<()> {
        let key = uuid_string(id);
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM documents_fts WHERE rowid IN (SELECT id FROM documents WHERE space = ?)", [&key])?;
        transaction.execute("DELETE FROM links WHERE document_id IN (SELECT id FROM documents WHERE space = ?)", [&key])?;
        transaction.execute("DELETE FROM documents WHERE space = ?", [&key])?;
        transaction.commit()?;
        Ok(())
    }
}

/// Поиск по заголовкам в памяти — для запросов короче трёх букв, которые не умеет полнотекстовый индекс.
pub fn title_search(query: &str, spaces: &[&SpaceSnapshot], limit: usize) -> Vec<SearchHit> {
    let needle = title_key(query);
    if needle.is_empty() {
        return Vec::new();
    }
    let mut matches: Vec<(&SpaceSnapshot, &PageNode, usize)> = spaces
        .iter()
        .flat_map(|snapshot| snapshot.pages.values().map(move |node| (*snapshot, node)))
        .filter(|(_, node)| title_key(&node.title).contains(&needle))
        .map(|(snapshot, node)| (snapshot, node, node.title.chars().count()))
        .collect();
    matches.sort_by(|lhs, rhs| lhs.2.cmp(&rhs.2).then_with(|| natural_cmp(&lhs.1.title, &rhs.1.title)));
    matches
        .into_iter()
        .take(limit)
        .map(|(snapshot, node, _)| SearchHit { page: snapshot.page_ref(&node.path), title: node.title.clone(), snippet: String::new() })
        .collect()
}

fn migrate(connection: &Connection) -> Result<()> {
    let version: i32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version != SCHEMA_VERSION {
        connection.execute_batch("DROP TABLE IF EXISTS links; DROP TABLE IF EXISTS documents_fts; DROP TABLE IF EXISTS documents;")?;
    }
    connection.execute_batch(&format!(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         CREATE TABLE IF NOT EXISTS documents (
             id INTEGER PRIMARY KEY,
             space TEXT NOT NULL,
             path TEXT NOT NULL,
             title TEXT NOT NULL,
             mtime REAL NOT NULL,
             UNIQUE (space, path)
         );
         CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(title, labels, body, tokenize = 'trigram');
         CREATE TABLE IF NOT EXISTS links (
             document_id INTEGER NOT NULL,
             target_space TEXT,
             target_title TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS links_target ON links (target_title);
         CREATE INDEX IF NOT EXISTS links_document ON links (document_id);
         PRAGMA user_version = {SCHEMA_VERSION};"
    ))?;
    Ok(())
}

fn known_documents(transaction: &Transaction<'_>, space_key: &str, only: Option<&[String]>) -> Result<FxHashMap<String, (i64, f64)>> {
    let mut known = FxHashMap::default();
    match only {
        None => {
            let mut statement = transaction.prepare_cached("SELECT path, id, mtime FROM documents WHERE space = ?")?;
            let rows = statement.query_map([space_key], |row| Ok((row.get::<_, String>(0)?, (row.get(1)?, row.get(2)?))))?;
            for row in rows {
                let (path, entry) = row?;
                known.entry(path).or_insert(entry);
            }
        }
        Some(paths) => {
            let mut statement = transaction.prepare_cached("SELECT id, mtime FROM documents WHERE space = ? AND path = ?")?;
            for path in paths {
                if let Some(entry) = statement.query_row(params![space_key, path], |row| Ok((row.get(0)?, row.get(1)?))).optional()? {
                    known.insert(path.clone(), entry);
                }
            }
        }
    }
    Ok(known)
}

fn upsert(transaction: &Transaction<'_>, space_key: &str, change: &Pending<'_>, body: &str, targets: &[(Option<String>, String)]) -> Result<()> {
    let id = match change.existing_id {
        Some(id) => {
            transaction.prepare_cached("UPDATE documents SET title = ?, mtime = ? WHERE id = ?")?.execute(params![
                change.node.title,
                change.node.modified,
                id
            ])?;
            transaction.prepare_cached("DELETE FROM documents_fts WHERE rowid = ?")?.execute([id])?;
            transaction.prepare_cached("DELETE FROM links WHERE document_id = ?")?.execute([id])?;
            id
        }
        None => {
            transaction.prepare_cached("INSERT INTO documents (space, path, title, mtime) VALUES (?, ?, ?, ?)")?.execute(params![
                space_key,
                change.path,
                change.node.title,
                change.node.modified
            ])?;
            transaction.last_insert_rowid()
        }
    };
    transaction.prepare_cached("INSERT INTO documents_fts (rowid, title, labels, body) VALUES (?, ?, ?, ?)")?.execute(params![
        id,
        change.node.title,
        change.node.labels.join(" "),
        body
    ])?;
    let mut insert_link = transaction.prepare_cached("INSERT INTO links (document_id, target_space, target_title) VALUES (?, ?, ?)")?;
    for (space, title) in targets {
        insert_link.execute(params![id, space, title])?;
    }
    Ok(())
}

fn delete_document(transaction: &Transaction<'_>, id: i64) -> Result<()> {
    transaction.prepare_cached("DELETE FROM documents_fts WHERE rowid = ?")?.execute([id])?;
    transaction.prepare_cached("DELETE FROM links WHERE document_id = ?")?.execute([id])?;
    transaction.prepare_cached("DELETE FROM documents WHERE id = ?")?.execute([id])?;
    Ok(())
}

fn hit(row: &rusqlite::Row<'_>) -> rusqlite::Result<Option<SearchHit>> {
    let space: String = row.get(0)?;
    let Ok(space_id) = Uuid::parse_str(&space) else { return Ok(None) };
    Ok(Some(SearchHit { page: PageRef::new(space_id, row.get::<_, String>(1)?), title: row.get(2)?, snippet: row.get(3)? }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::SpaceScanner;
    use crate::tests_support::Fixture;

    #[test]
    fn finds_cyrillic_by_part_of_word() {
        let folder = tempfile::tempdir().unwrap();
        let space = Fixture::space("Работа", folder.path());
        Fixture::write_page(&space, "Интеграция", Some("Интеграция с CRM"), &[], "Требования к ВЫГРУЗКЕ заказов в хранилище");
        let mut index = SearchIndex::open(&folder.path().join("index.sqlite")).unwrap();
        index.sync(&SpaceScanner::scan(&space)).unwrap();

        let hits = index.search("выгрузк", None, 50).unwrap();

        assert_eq!(hits.iter().map(|hit| hit.title.as_str()).collect::<Vec<_>>(), ["Интеграция с CRM"]);
        assert!(hits[0].snippet.contains(HIGHLIGHT_START));
        assert!(index.search("ин", None, 50).unwrap().is_empty());
    }

    #[test]
    fn incremental_sync() {
        let folder = tempfile::tempdir().unwrap();
        let space = Fixture::space("Работа", folder.path());
        Fixture::write_page(&space, "A", Some("A"), &[], "Первый текст");
        Fixture::write_page(&space, "B", Some("B"), &[], "Бета текст");
        let mut index = SearchIndex::open(&folder.path().join("index.sqlite")).unwrap();

        assert_eq!(index.sync(&SpaceScanner::scan(&space)).unwrap(), 3);
        assert_eq!(index.sync(&SpaceScanner::scan(&space)).unwrap(), 0);

        std::thread::sleep(Duration::from_millis(20));
        Fixture::write_page(&space, "A", Some("A"), &[], "Новый текст про бюджет");
        assert_eq!(index.sync(&SpaceScanner::scan(&space)).unwrap(), 1);
        assert_eq!(index.search("бюджет", None, 50).unwrap().len(), 1);

        fs::remove_dir_all(space.root.join("B")).unwrap();
        assert_eq!(index.sync(&SpaceScanner::scan(&space)).unwrap(), 1);
        assert!(index.search("Бета", None, 50).unwrap().is_empty());
    }

    #[test]
    fn sync_of_one_page() {
        let folder = tempfile::tempdir().unwrap();
        let space = Fixture::space("Работа", folder.path());
        Fixture::write_page(&space, "A", Some("A"), &[], "Первый текст");
        let mut index = SearchIndex::open(&folder.path().join("index.sqlite")).unwrap();
        index.sync(&SpaceScanner::scan(&space)).unwrap();

        std::thread::sleep(Duration::from_millis(20));
        Fixture::write_page(&space, "A", Some("A"), &[], "Про отпуск");
        let snapshot = SpaceScanner::scan(&space);
        assert_eq!(index.sync_pages(&snapshot, &["A".to_owned()]).unwrap(), 1);
        assert_eq!(index.search("отпуск", None, 50).unwrap().len(), 1);
        assert_eq!(index.sync(&snapshot).unwrap(), 0);
    }

    #[test]
    fn backlinks_within_and_across_spaces() {
        let folder = tempfile::tempdir().unwrap();
        let work = Fixture::space("Работа", folder.path());
        let personal = Fixture::space("Личное", folder.path());
        Fixture::write_page(&work, "Требования", Some("Требования"), &[], "");
        Fixture::write_page(&work, "Встреча", Some("Встреча"), &[], "[[Требования]]");
        Fixture::write_page(&personal, "Заметка", Some("Заметка"), &[], "[[Работа::требования]]");
        Fixture::write_page(&personal, "Другая", Some("Другая"), &[], "[[Требования]]");
        let mut index = SearchIndex::open(&folder.path().join("index.sqlite")).unwrap();
        index.sync(&SpaceScanner::scan(&work)).unwrap();
        index.sync(&SpaceScanner::scan(&personal)).unwrap();

        let mut titles: Vec<String> = index.backlinks("Требования", &work).unwrap().into_iter().map(|hit| hit.title).collect();
        titles.sort();
        assert_eq!(titles, ["Встреча", "Заметка"]);
    }

    #[test]
    fn short_query_uses_titles() {
        let folder = tempfile::tempdir().unwrap();
        let space = Fixture::space("Работа", folder.path());
        Fixture::write_page(&space, "ТЗ", Some("ТЗ на CRM"), &[], "");
        Fixture::write_page(&space, "Другое", Some("Другое"), &[], "");
        let snapshot = SpaceScanner::scan(&space);

        let hits = title_search("тз", &[&snapshot], 10);

        assert_eq!(hits.iter().map(|hit| hit.title.as_str()).collect::<Vec<_>>(), ["ТЗ на CRM"]);
    }
}
