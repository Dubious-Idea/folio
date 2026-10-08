use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{FolioError, IoContext, Result};
use crate::fsutil;
use crate::space::Space;
use crate::text::title_key;

pub struct FolioPaths;

impl FolioPaths {
    /// Служебная папка: реестр пространств и индекс поиска. `FOLIO_SUPPORT_DIR` подменяет её для проверок.
    pub fn support_folder() -> PathBuf {
        if let Some(path) = std::env::var_os("FOLIO_SUPPORT_DIR").filter(|path| !path.is_empty()) {
            return PathBuf::from(path);
        }
        dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("Folio")
    }

    /// Пространства по умолчанию — в домашней папке, как и на macOS: `C:\Users\<имя>\Folio`.
    pub fn default_spaces_folder() -> PathBuf {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join("Folio")
    }

    pub fn registry_path() -> PathBuf {
        Self::support_folder().join("spaces.json")
    }

    pub fn index_path() -> PathBuf {
        Self::support_folder().join("index.sqlite")
    }
}

#[derive(Serialize, Deserialize, Default)]
struct StoredList {
    spaces: Vec<String>,
}

pub struct LoadResult {
    pub spaces: Vec<Space>,
    pub missing_paths: Vec<String>,
}

/// Список подключённых пространств, общий для приложения и утилиты `folio`.
pub struct SpaceRegistry {
    pub file: PathBuf,
}

impl Default for SpaceRegistry {
    fn default() -> Self {
        SpaceRegistry { file: FolioPaths::registry_path() }
    }
}

impl SpaceRegistry {
    pub fn new(file: PathBuf) -> Self {
        SpaceRegistry { file }
    }

    pub fn load(&self) -> LoadResult {
        let mut spaces = Vec::new();
        let mut missing_paths = Vec::new();
        for path in self.stored_paths() {
            match Space::open(Path::new(&path)) {
                Ok(space) => spaces.push(space),
                Err(_) => missing_paths.push(path),
            }
        }
        LoadResult { spaces, missing_paths }
    }

    pub fn register(&self, space: &Space) -> Result<()> {
        let name_key = title_key(space.name());
        if let Some(clash) = self.load().spaces.into_iter().find(|other| other.id() == space.id() || title_key(other.name()) == name_key) {
            return Err(FolioError::SpaceAlreadyRegistered(clash.name().to_owned()));
        }
        let mut paths = self.stored_paths();
        paths.push(space.root_path());
        self.save(paths)
    }

    pub fn unregister_id(&self, id: Uuid) -> Result<()> {
        let paths = self.stored_paths().into_iter().filter(|path| Space::open(Path::new(path)).map_or(true, |space| space.id() != id)).collect();
        self.save(paths)
    }

    pub fn unregister_path(&self, path: &str) -> Result<()> {
        let paths = self.stored_paths().into_iter().filter(|stored| stored != path).collect();
        self.save(paths)
    }

    fn stored_paths(&self) -> Vec<String> {
        fs::read(&self.file)
            .ok()
            .and_then(|data| serde_json::from_slice::<StoredList>(&data).ok())
            .map(|list| list.spaces)
            .unwrap_or_default()
    }

    fn save(&self, spaces: Vec<String>) -> Result<()> {
        if let Some(folder) = self.file.parent() {
            fs::create_dir_all(folder).at(folder)?;
        }
        let json = serde_json::to_string_pretty(&StoredList { spaces }).expect("список сериализуется");
        fsutil::write_atomic(&self.file, json.as_bytes()).at(&self.file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::SpaceManifest;
    use crate::tests_support::Fixture;

    #[test]
    fn registers_spaces() {
        let folder = tempfile::tempdir().unwrap();
        let registry = SpaceRegistry::new(folder.path().join("spaces.json"));
        let work = Fixture::space("Работа", folder.path());
        registry.register(&work).unwrap();
        let names: Vec<String> = registry.load().spaces.iter().map(|space| space.name().to_owned()).collect();
        assert_eq!(names, ["Работа"]);

        let clone = Space::new(SpaceManifest::new("работа", "folder"), &folder.path().join("Копия"));
        assert!(matches!(registry.register(&clone), Err(FolioError::SpaceAlreadyRegistered(name)) if name == "Работа"));

        fs::remove_dir_all(&work.root).unwrap();
        assert_eq!(registry.load().missing_paths, [work.root_path()]);
        registry.unregister_path(&work.root_path()).unwrap();
        assert!(registry.load().missing_paths.is_empty());
    }
}
