//! Ядро Folio для Windows: тот же формат пространств, что и у версии для macOS.
//!
//! Пространство — папка со страницами, своей историей git и манифестом `.folio/space.json`.
//! Страница — папка с `index.md`: YAML-шапка и Markdown-тело.

pub mod attachments;
pub mod bootstrap;
pub mod checker;
mod content;
pub mod error;
pub mod frontmatter;
pub mod fsutil;
pub mod git;
pub mod index;
pub mod links;
pub mod operations;
pub mod page_writer;
pub mod registry;
pub mod space;
pub mod text;

#[cfg(test)]
mod tests_support;

pub use error::{FolioError, Result};
pub use frontmatter::{FrontmatterWriter, PageDocument, PageFrontmatter, PageStatus, StatusColor};
pub use links::{AttachmentReferences, PageLink, WikiLink, WikiLinkParser};
pub use space::{LibrarySnapshot, PageNode, PageRef, Space, SpaceManifest, SpaceScanner, SpaceSnapshot};
