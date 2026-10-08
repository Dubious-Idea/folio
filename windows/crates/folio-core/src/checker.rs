//! Проверка пространства на то, что ломает вики: дубли, битые ссылки, потерянные вложения.

use std::fs;
use std::path::Path;

use rayon::prelude::*;
use rustc_hash::FxHashSet;

use crate::frontmatter::PageDocument;
use crate::fsutil;
use crate::links::{AttachmentReferences, WikiLinkParser};
use crate::space::{LibrarySnapshot, SpaceScanner, SpaceSnapshot};
use crate::text::PAGE_FILE_NAME;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IssueKind {
    DuplicateTitle,
    BrokenLink,
    MissingAttachment,
    InvalidFrontmatter,
    MissingFrontmatter,
    StrayFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckIssue {
    pub kind: IssueKind,
    /// Путь страницы; у главной — «(главная)».
    pub path: String,
    pub message: String,
}

const HOME_LABEL: &str = "(главная)";
const IGNORED_MARKDOWN: [&str; 2] = [PAGE_FILE_NAME, "CLAUDE.md"];

pub fn check(snapshot: &SpaceSnapshot, library: &LibrarySnapshot) -> Vec<CheckIssue> {
    let mut issues: Vec<CheckIssue> = snapshot
        .duplicate_titles()
        .into_iter()
        .map(|(title, paths)| CheckIssue {
            kind: IssueKind::DuplicateTitle,
            path: paths.iter().map(|path| display_path(path)).collect::<Vec<_>>().join(", "),
            message: format!("заголовок «{title}» носят несколько страниц"),
        })
        .collect();
    let mut paths: Vec<&String> = snapshot.pages.keys().collect();
    paths.sort();
    let page_issues: Vec<Vec<CheckIssue>> = paths.par_iter().map(|path| issues_for_page(path, snapshot, library)).collect();
    issues.extend(page_issues.into_iter().flatten());
    issues
}

fn display_path(path: &str) -> &str {
    if path.is_empty() { HOME_LABEL } else { path }
}

fn issues_for_page(path: &str, snapshot: &SpaceSnapshot, library: &LibrarySnapshot) -> Vec<CheckIssue> {
    let shown = display_path(path).to_owned();
    let issue = |kind, message: String| CheckIssue { kind, path: shown.clone(), message };
    let Ok(text) = fs::read_to_string(snapshot.file_path(path)) else {
        return vec![issue(IssueKind::InvalidFrontmatter, "файл не читается как UTF-8".to_owned())];
    };
    let document = PageDocument::parse(&text);
    let mut issues = Vec::new();

    if let Some(error) = &document.frontmatter_error {
        issues.push(issue(IssueKind::InvalidFrontmatter, format!("шапка не разбирается: {error}")));
    } else if document.frontmatter.id.is_none() || document.frontmatter.title.is_none() {
        issues.push(issue(IssueKind::MissingFrontmatter, "в шапке нет id или title".to_owned()));
    }

    let mut broken: Vec<String> = WikiLinkParser::links(&document.body)
        .into_iter()
        .collect::<FxHashSet<_>>()
        .into_iter()
        .filter(|link| library.resolve(link, snapshot.space.id()).is_none())
        .map(|link| link.markdown())
        .collect();
    broken.sort();
    issues.extend(broken.into_iter().map(|link| issue(IssueKind::BrokenLink, format!("битая ссылка {link}"))));

    let folder = snapshot.folder_path(path);
    let mut missing: Vec<String> = AttachmentReferences::relative_paths(&document.body)
        .into_iter()
        .collect::<FxHashSet<_>>()
        .into_iter()
        .filter(|relative| !fsutil::join_segments(&folder, relative).exists())
        .collect();
    missing.sort();
    issues.extend(missing.into_iter().map(|file| issue(IssueKind::MissingAttachment, format!("нет файла вложения {file}"))));

    issues.extend(stray_files(&folder, &shown));
    issues
}

fn stray_files(folder: &Path, shown: &str) -> Vec<CheckIssue> {
    let mut entries: Vec<(String, bool)> = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let metadata = entry.metadata().ok()?;
            (!fsutil::is_hidden(&name, &metadata)).then_some((name, metadata.is_dir()))
        })
        .collect();
    entries.sort();
    entries
        .into_iter()
        .filter_map(|(name, is_directory)| {
            let issue = |message: String| CheckIssue { kind: IssueKind::StrayFile, path: shown.to_owned(), message };
            if !is_directory {
                let is_markdown = Path::new(&name).extension().is_some_and(|extension| extension == "md");
                let ignored = IGNORED_MARKDOWN.iter().any(|ignored| ignored.eq_ignore_ascii_case(&name));
                return (is_markdown && !ignored).then(|| issue(format!("файл {name} не страница: страница — папка с index.md")));
            }
            let entry = folder.join(&name);
            if name.starts_with('_') || SpaceScanner::is_page_folder(&entry) {
                return None;
            }
            let has_markdown = fs::read_dir(&entry)
                .into_iter()
                .flatten()
                .flatten()
                .any(|inner| inner.file_name().to_string_lossy().ends_with(".md"));
            has_markdown.then(|| issue(format!("в папке {name} есть Markdown, но нет index.md — в дереве её не видно")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::Fixture;
    use std::sync::Arc;

    #[test]
    fn finds_problems() {
        let folder = tempfile::tempdir().unwrap();
        let space = Fixture::space("Работа", folder.path());
        Fixture::write_page(&space, "Страница", Some("Страница"), &[], "[[Нет такой]] ![](схема.png) [[Работа]]");
        fs::write(space.root.join("Страница").join("заметки.md"), "черновик").unwrap();
        let snapshot = Arc::new(SpaceScanner::scan(&space));

        let issues = check(&snapshot, &LibrarySnapshot::new(vec![snapshot.clone()]));

        let kinds: Vec<IssueKind> = issues.iter().map(|issue| issue.kind).collect();
        assert_eq!(kinds, [IssueKind::BrokenLink, IssueKind::MissingAttachment, IssueKind::StrayFile]);
        assert!(issues.iter().all(|issue| issue.path == "Страница"));
    }
}
