//! Наблюдение за папками пространств (ReadDirectoryChangesW) и склейка событий в пачки.

use std::path::{Path, PathBuf};
use std::sync::{Weak, mpsc};
use std::time::{Duration, Instant};

use folio_core::fsutil;
use notify::{EventKind, RecursiveMode, Watcher};
use rustc_hash::FxHashMap;
use uuid::Uuid;

use crate::library::Library;

pub struct Change {
    space: Uuid,
    paths: Vec<PathBuf>,
}

pub struct SpaceWatcher {
    _watcher: notify::RecommendedWatcher,
}

impl SpaceWatcher {
    pub fn start(space: Uuid, root: &Path, sender: mpsc::Sender<Change>) -> Option<SpaceWatcher> {
        let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            let Ok(event) = result else { return };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            let paths: Vec<PathBuf> = event.paths.into_iter().filter(|path| is_relevant(path)).collect();
            if !paths.is_empty() {
                let _ = sender.send(Change { space, paths });
            }
        })
        .ok()?;
        watcher.watch(root, RecursiveMode::Recursive).ok()?;
        Some(SpaceWatcher { _watcher: watcher })
    }
}

/// Служебные файлы git, временные файлы записи и эскизы Проводника дерево не меняют.
fn is_relevant(path: &Path) -> bool {
    !path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        name == ".git" || name.starts_with(fsutil::TEMPORARY_PREFIX) || name.starts_with(fsutil::RENAME_PREFIX) || fsutil::is_system_file(&name)
    })
}

/// Тишина, после которой пачка уходит в обработку, и предел ожидания при непрерывных событиях.
const QUIET: Duration = Duration::from_millis(120);
const MAXIMUM_DELAY: Duration = Duration::from_millis(600);

pub fn spawn_debouncer(library: Weak<Library>, receiver: mpsc::Receiver<Change>) {
    std::thread::Builder::new()
        .name("folio-watcher".to_owned())
        .spawn(move || {
            while let Ok(first) = receiver.recv() {
                let started = Instant::now();
                let mut batch: FxHashMap<Uuid, Vec<PathBuf>> = FxHashMap::default();
                batch.entry(first.space).or_default().extend(first.paths);
                loop {
                    let remaining = MAXIMUM_DELAY.saturating_sub(started.elapsed()).min(QUIET);
                    if remaining.is_zero() {
                        break;
                    }
                    match receiver.recv_timeout(remaining) {
                        Ok(change) => batch.entry(change.space).or_default().extend(change.paths),
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
                let Some(library) = library.upgrade() else { return };
                for (space, mut paths) in batch {
                    paths.sort();
                    paths.dedup();
                    library.handle_paths(space, paths);
                }
            }
        })
        .expect("поток наблюдения за папками");
}
