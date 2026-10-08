use std::fs;
use std::path::Path;

use chrono::{DateTime, Local};

use crate::content;
use crate::error::{FolioError, IoContext, Result};
use crate::fsutil;
use crate::git::GitRepository;
use crate::space::{Space, SpaceManifest};
use crate::text::validate_title;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceKind {
    Work,
    Personal,
    Blank,
}

impl SpaceKind {
    /// Значок из SF Symbols: так же записывает версия для macOS, Windows показывает его своим рисунком.
    pub fn default_icon(self) -> &'static str {
        match self {
            SpaceKind::Work => "briefcase",
            SpaceKind::Personal => "person",
            SpaceKind::Blank => "folder",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "work" => Some(SpaceKind::Work),
            "personal" => Some(SpaceKind::Personal),
            "blank" => Some(SpaceKind::Blank),
            _ => None,
        }
    }
}

/// Создаёт пространство в пустой папке: манифест, главную, шаблоны, инструкцию и первый снимок в git.
pub fn create_space(root: &Path, raw_name: &str, icon: Option<&str>, kind: SpaceKind, now: DateTime<Local>) -> Result<Space> {
    let name = validate_title(raw_name)?;
    if root.exists() {
        let has_content = fs::read_dir(root)
            .at(root)?
            .flatten()
            .any(|entry| !fsutil::is_system_file(&entry.file_name().to_string_lossy()));
        if has_content {
            return Err(FolioError::FolderNotEmpty(fsutil::display_path(root)));
        }
    }

    let mut manifest = SpaceManifest::new(&name, icon.unwrap_or(kind.default_icon()));
    manifest.created = now.with_timezone(&chrono::Utc);
    let space = Space::new(manifest, root);
    space.write_manifest()?;
    for file in content::files(kind, &name, now) {
        let path = fsutil::join_segments(&space.root, file.path);
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder).at(folder)?;
        }
        fsutil::write_atomic(&path, file.contents.as_bytes()).at(&path)?;
    }

    let repository = GitRepository::new(&space.root);
    repository.initialize()?;
    repository.commit_all(&format!("Создано пространство «{name}»"))?;
    Ok(space)
}
