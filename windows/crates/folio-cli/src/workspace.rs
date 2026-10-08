use std::fmt;
use std::sync::{Arc, OnceLock};

use folio_core::index::SearchIndex;
use folio_core::registry::{FolioPaths, SpaceRegistry};
use folio_core::text::title_key;
use folio_core::{FolioError, LibrarySnapshot, PageNode, PageRef, Space, SpaceSnapshot};

pub enum Failure {
    Folio(FolioError),
    Usage(String),
    Output(std::io::Error),
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Folio(error) => write!(formatter, "{error}"),
            Failure::Usage(message) => write!(formatter, "{message}"),
            Failure::Output(error) => write!(formatter, "вывод: {error}"),
        }
    }
}

impl From<FolioError> for Failure {
    fn from(error: FolioError) -> Self {
        Failure::Folio(error)
    }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Failure::Output(error)
    }
}

/// Подключённые пространства. Сканируются по требованию: большинству команд нужно одно пространство,
/// и утилита, которую скрипты зовут десятки раз подряд, не обходит лишние папки.
pub struct Workspace {
    spaces: Vec<Space>,
    snapshots: Vec<OnceLock<Arc<SpaceSnapshot>>>,
}

impl Workspace {
    pub fn load() -> Workspace {
        let spaces = SpaceRegistry::default().load().spaces;
        let snapshots = spaces.iter().map(|_| OnceLock::new()).collect();
        Workspace { spaces, snapshots }
    }

    fn snapshot_at(&self, index: usize) -> Arc<SpaceSnapshot> {
        self.snapshots[index].get_or_init(|| Arc::new(folio_core::SpaceScanner::scan(&self.spaces[index]))).clone()
    }

    /// Все пространства; ещё не отсканированные обходятся параллельно.
    pub fn library(&self) -> LibrarySnapshot {
        let missing: Vec<usize> = (0..self.spaces.len()).filter(|index| self.snapshots[*index].get().is_none()).collect();
        let fresh = LibrarySnapshot::scan(&missing.iter().map(|index| self.spaces[*index].clone()).collect::<Vec<_>>());
        for (index, snapshot) in missing.into_iter().zip(fresh.spaces) {
            let _ = self.snapshots[index].set(snapshot);
        }
        LibrarySnapshot::new((0..self.spaces.len()).map(|index| self.snapshot_at(index)).collect())
    }

    pub fn space(&self, name: Option<&str>) -> Result<Arc<SpaceSnapshot>, Failure> {
        if let Some(name) = name {
            let key = title_key(name);
            let index = self
                .spaces
                .iter()
                .position(|space| title_key(space.name()) == key)
                .ok_or_else(|| FolioError::SpaceNotFound(name.to_owned()))?;
            return Ok(self.snapshot_at(index));
        }
        if let Ok(current) = std::env::current_dir() {
            if let Some(index) = self.spaces.iter().position(|space| space.contains(&current)) {
                return Ok(self.snapshot_at(index));
            }
        }
        match self.spaces.len() {
            1 => Ok(self.snapshot_at(0)),
            0 => Err(Failure::Usage("Пространств нет. Создай его: folio init <папка> --name <имя>".to_owned())),
            _ => {
                let names: Vec<&str> = self.spaces.iter().map(Space::name).collect();
                Err(Failure::Usage(format!("Укажи пространство через --space: {}", names.join(", "))))
            }
        }
    }

    /// Страница по заголовку, а если заголовок не нашёлся — по пути папки.
    pub fn page<'a>(&self, reference: &str, snapshot: &'a SpaceSnapshot) -> Result<&'a PageNode, Failure> {
        match snapshot.unique_page(reference) {
            Ok(page) => Ok(page),
            Err(error) => {
                let path = reference.replace('\\', "/");
                let path = path.trim_matches('/');
                match snapshot.page(path) {
                    Some(page) if !path.is_empty() => Ok(page),
                    _ => Err(error.into()),
                }
            }
        }
    }

    pub fn synced_index(&self) -> Result<SearchIndex, Failure> {
        let mut index = SearchIndex::open(&FolioPaths::index_path())?;
        for snapshot in self.library().spaces {
            index.sync(&snapshot)?;
        }
        Ok(index)
    }

    pub fn describe(&self, page: &PageRef) -> String {
        let Some(space) = self.spaces.iter().find(|space| space.id() == page.space_id) else { return page.path.clone() };
        if page.path.is_empty() { space.name().to_owned() } else { format!("{} / {}", space.name(), page.path) }
    }
}
