//! Общие заготовки для тестов: пространства без git во временной папке.

use std::fs;
use std::path::{Path, PathBuf};

use crate::frontmatter::FrontmatterWriter;
use crate::space::{Space, SpaceManifest};
use crate::text::{PAGE_FILE_NAME, make_page_id};

pub struct Fixture;

impl Fixture {
    /// Пространство без git: манифест и главная страница.
    pub fn space(name: &str, folder: &Path) -> Space {
        let space = Space::new(SpaceManifest::new(name, "folder"), &folder.join(name));
        space.write_manifest().unwrap();
        Self::write_page(&space, "", Some(name), &[], "");
        space
    }

    pub fn write_page(space: &Space, path: &str, title: Option<&str>, extra: &[&str], body: &str) {
        let folder = Self::folder(space, path);
        fs::create_dir_all(&folder).unwrap();
        let mut lines = vec![format!("id: {}", make_page_id())];
        if let Some(title) = title {
            lines.push(format!("title: {}", FrontmatterWriter::scalar(title)));
        }
        lines.extend(extra.iter().map(|line| (*line).to_owned()));
        fs::write(folder.join(PAGE_FILE_NAME), FrontmatterWriter::document(&lines, body)).unwrap();
    }

    pub fn read(space: &Space, path: &str) -> String {
        fs::read_to_string(Self::folder(space, path).join(PAGE_FILE_NAME)).unwrap()
    }

    pub fn folder(space: &Space, path: &str) -> PathBuf {
        crate::fsutil::join_segments(&space.root, path)
    }
}
