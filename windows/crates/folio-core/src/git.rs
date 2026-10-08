//! История пространства в git: снимки, журнал страницы и её старые версии.
//!
//! Git встроен (libgit2): не нужно ставить Git для Windows, и открытие страницы
//! не запускает процессов. Репозитории обычные — их видит и `git` в терминале.

use std::path::{Path, PathBuf};
use std::sync::Once;

use chrono::{DateTime, FixedOffset, TimeZone};
use git2::{DiffFindOptions, IndexAddOption, Repository, RepositoryInitOptions, Signature, Sort, StatusOptions};

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRevision {
    pub hash: String,
    pub date: DateTime<FixedOffset>,
    pub author: String,
    pub message: String,
    /// Путь файла в этой версии: после переименований он отличается от текущего.
    pub path: String,
}

impl GitRevision {
    pub fn short_hash(&self) -> &str {
        &self.hash[..self.hash.len().min(7)]
    }
}

pub struct GitRepository {
    pub root: PathBuf,
}

const FALLBACK_NAME: &str = "Folio";
const FALLBACK_EMAIL: &str = "folio@localhost";

/// libgit2 по умолчанию отказывается открывать репозиторий чужого владельца — как git с его
/// `safe.directory`. Защита нужна против хуков и fsmonitor из чужого конфига, а libgit2 их не запускает.
/// Без этого пространства на дисках после переустановки Windows (с прежним SID владельца) не открывались бы.
fn configure_libgit2() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let _ = git2::opts::set_verify_owner_validation(false);
    });
}

impl GitRepository {
    pub fn new(root: &Path) -> Self {
        configure_libgit2();
        GitRepository { root: root.to_path_buf() }
    }

    pub fn is_initialized(&self) -> bool {
        self.root.join(".git").exists()
    }

    fn open(&self) -> Result<Repository> {
        Ok(Repository::open(&self.root)?)
    }

    pub fn initialize(&self) -> Result<()> {
        if self.is_initialized() {
            return Ok(());
        }
        let mut options = RepositoryInitOptions::new();
        options.initial_head("main");
        let repository = Repository::init_opts(&self.root, &options)?;
        let has_email = repository.config()?.get_string("user.email").is_ok_and(|email| !email.is_empty());
        if !has_email {
            let mut config = repository.config()?.open_level(git2::ConfigLevel::Local)?;
            config.set_str("user.name", FALLBACK_NAME)?;
            config.set_str("user.email", FALLBACK_EMAIL)?;
        }
        Ok(())
    }

    /// Есть ли изменения, которых нет в истории, включая новые файлы.
    pub fn has_changes(&self) -> Result<bool> {
        let repository = self.open()?;
        let mut options = StatusOptions::new();
        options.include_untracked(true).recurse_untracked_dirs(true).exclude_submodules(true).include_ignored(false);
        Ok(!repository.statuses(Some(&mut options))?.is_empty())
    }

    /// Сохраняет все изменения одним снимком; `false`, если сохранять нечего.
    pub fn commit_all(&self, message: &str) -> Result<bool> {
        let repository = self.open()?;
        let mut index = repository.index()?;
        index.add_all(["*"], IndexAddOption::DEFAULT, None)?;
        index.update_all(["*"], None)?;
        index.write()?;
        let tree_id = index.write_tree()?;

        let parent = repository.head().ok().and_then(|head| head.peel_to_commit().ok());
        let unchanged = match &parent {
            Some(parent) => parent.tree_id() == tree_id,
            None => index.is_empty(),
        };
        if unchanged {
            return Ok(false);
        }
        let tree = repository.find_tree(tree_id)?;
        let signature = repository.signature().or_else(|_| Signature::now(FALLBACK_NAME, FALLBACK_EMAIL))?;
        let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();
        repository.commit(Some("HEAD"), &signature, &signature, message, &tree, &parents)?;
        Ok(true)
    }

    /// Журнал файла с переименованиями, как `git log --follow`.
    pub fn history(&self, relative_path: &str, limit: usize) -> Result<Vec<GitRevision>> {
        if !self.is_initialized() {
            return Ok(Vec::new());
        }
        let repository = self.open()?;
        let Ok(head) = repository.head() else { return Ok(Vec::new()) };
        let Some(head_id) = head.target() else { return Ok(Vec::new()) };

        let mut walk = repository.revwalk()?;
        walk.push(head_id)?;
        walk.set_sorting(Sort::TIME)?;

        let mut path = relative_path.to_owned();
        let mut result = Vec::new();
        for id in walk {
            let commit = repository.find_commit(id?)?;
            let tree = commit.tree()?;
            let current = tree.get_path(Path::new(&path)).ok().map(|entry| entry.id());

            let parent_trees: Vec<git2::Tree<'_>> = commit.parents().filter_map(|parent| parent.tree().ok()).collect();
            let parent_entries: Vec<Option<git2::Oid>> =
                parent_trees.iter().map(|parent| parent.get_path(Path::new(&path)).ok().map(|entry| entry.id())).collect();

            // Файл такой же, как у родителя (или его нет у обоих) — снимок файл не трогал.
            // У слияния достаточно совпасть с одним из родителей, как в упрощённой истории git.
            if parent_entries.iter().any(|entry| *entry == current) {
                continue;
            }
            if current.is_none() && parent_entries.is_empty() {
                continue;
            }

            let recorded_path = path.clone();
            if current.is_some() && matches!(parent_entries.first(), Some(None)) {
                if let Some(old_path) = renamed_from(&repository, &parent_trees[0], &tree, &path) {
                    path = old_path;
                }
            }
            result.push(revision(&commit, recorded_path));
            if result.len() >= limit {
                break;
            }
        }
        Ok(result)
    }

    /// Текст файла в старой версии.
    pub fn contents(&self, relative_path: &str, hash: &str) -> Result<String> {
        let repository = self.open()?;
        let commit = repository.find_commit(git2::Oid::from_str(hash)?)?;
        let entry = commit.tree()?.get_path(Path::new(relative_path))?;
        let blob = repository.find_blob(entry.id())?;
        Ok(String::from_utf8_lossy(blob.content()).into_owned())
    }
}

/// Старый путь, если файл в этом снимке переименовали (сходство больше половины, как у git).
fn renamed_from(repository: &Repository, parent: &git2::Tree<'_>, tree: &git2::Tree<'_>, path: &str) -> Option<String> {
    let mut diff = repository.diff_tree_to_tree(Some(parent), Some(tree), None).ok()?;
    let mut options = DiffFindOptions::new();
    options.renames(true);
    diff.find_similar(Some(&mut options)).ok()?;
    diff.deltas().find_map(|delta| {
        let is_rename = delta.status() == git2::Delta::Renamed;
        let new_path = delta.new_file().path()?.to_str()?;
        if is_rename && new_path == path { delta.old_file().path()?.to_str().map(str::to_owned) } else { None }
    })
}

fn revision(commit: &git2::Commit<'_>, path: String) -> GitRevision {
    let author = commit.author();
    let time = author.when();
    let offset = FixedOffset::east_opt(time.offset_minutes() * 60).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
    let date = offset.timestamp_opt(time.seconds(), 0).single().unwrap_or_else(|| offset.timestamp_opt(0, 0).unwrap());
    GitRevision {
        hash: commit.id().to_string(),
        date,
        author: String::from_utf8_lossy(author.name_bytes()).into_owned(),
        message: commit.summary_bytes().map(|summary| String::from_utf8_lossy(summary).into_owned()).unwrap_or_default(),
        path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap::{SpaceKind, create_space};
    use crate::checker;
    use crate::operations::PageOperations;
    use crate::space::{LibrarySnapshot, SpaceScanner};
    use crate::text::PAGE_FILE_NAME;
    use chrono::Local;
    use std::sync::Arc;

    #[test]
    fn bootstrap_creates_repository() {
        let folder = tempfile::tempdir().unwrap();
        let space = create_space(&folder.path().join("Работа"), "Работа", None, SpaceKind::Work, Local::now()).unwrap();
        let snapshot = Arc::new(SpaceScanner::scan(&space));
        let repository = GitRepository::new(&space.root);

        assert_eq!(snapshot.home().title, "Работа");
        assert!(snapshot.page("Справка по разметке").is_some());
        assert_eq!(PageOperations::new(&snapshot).template_names(), ["Протокол встречи", "Страница", "Техническое задание", "Требования"]);
        assert!(space.root.join("CLAUDE.md").exists());
        assert_eq!(repository.history("index.md", 200).unwrap().len(), 1);
        assert!(!repository.has_changes().unwrap());
        assert!(checker::check(&snapshot, &LibrarySnapshot::new(vec![snapshot.clone()])).is_empty());
    }

    #[test]
    fn history_follows_rename() {
        let folder = tempfile::tempdir().unwrap();
        let space = create_space(&folder.path().join("Личное"), "Личное", None, SpaceKind::Blank, Local::now()).unwrap();
        let repository = GitRepository::new(&space.root);
        let path = PageOperations::new(&SpaceScanner::scan(&space)).create_page("Идеи", "", None, Local::now()).unwrap();
        let file = space.root.join(&path).join(PAGE_FILE_NAME);
        let body = "\nСписок идей на осень: поехать в горы, дочитать книги, разобраться с оконными функциями в SQL.\n";
        let text = std::fs::read_to_string(&file).unwrap() + body;
        std::fs::write(&file, text).unwrap();
        repository.commit_all("Идеи").unwrap();

        let library = LibrarySnapshot::new(vec![Arc::new(SpaceScanner::scan(&space))]);
        let renamed = PageOperations::new(&library.spaces[0]).rename_page(&path, "Идеи на осень", &library).unwrap();
        repository.commit_all("Переименование").unwrap();

        let history = repository.history(&format!("{}/index.md", renamed.new_path), 200).unwrap();
        let messages: Vec<&str> = history.iter().map(|revision| revision.message.as_str()).collect();
        assert_eq!(messages, ["Переименование", "Идеи"]);
        assert_eq!(history.last().unwrap().path, "Идеи/index.md");
        let original = repository.contents(&history[1].path, &history[1].hash).unwrap();
        assert_eq!(crate::PageDocument::parse(&original).frontmatter.title.as_deref(), Some("Идеи"));
    }

    #[test]
    fn commit_reports_nothing_to_save() {
        let folder = tempfile::tempdir().unwrap();
        let space = create_space(&folder.path().join("Пустое"), "Пустое", None, SpaceKind::Blank, Local::now()).unwrap();
        let repository = GitRepository::new(&space.root);
        assert!(!repository.commit_all("ничего").unwrap());
        std::fs::remove_dir_all(space.root.join("_Входящие")).unwrap();
        assert!(repository.has_changes().unwrap());
        assert!(repository.commit_all("удалил входящие").unwrap());
        assert!(!repository.has_changes().unwrap());
    }
}
