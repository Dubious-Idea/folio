//! Файловые мелочи, которые на Windows устроены иначе, чем на macOS.

use std::fs::{self, File, Metadata};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

/// Префикс временных файлов атомарной записи; наблюдатель за папками их пропускает.
pub const TEMPORARY_PREFIX: &str = ".folio-tmp-";
/// Префикс временного имени при смене регистра папки.
pub const RENAME_PREFIX: &str = ".folio-rename-";

/// Служебные файлы систем, которые не считаются содержимым пространства.
pub const SYSTEM_FILES: [&str; 3] = [".DS_Store", "Thumbs.db", "desktop.ini"];

pub fn is_system_file(name: &str) -> bool {
    SYSTEM_FILES.iter().any(|system| system.eq_ignore_ascii_case(name))
}

/// Записывает файл целиком: сначала во временный рядом, потом подменой.
/// Если подменить не дают (файл открыт в другой программе без общего доступа), пишет поверх.
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let temporary = parent.join(format!("{TEMPORARY_PREFIX}{}", uuid::Uuid::new_v4().simple()));
    {
        let mut file = File::options().write(true).create_new(true).open(&temporary)?;
        file.write_all(contents)?;
    }
    let mut attempt = 0;
    loop {
        match fs::rename(&temporary, path) {
            Ok(()) => return Ok(()),
            Err(error) if attempt < 4 && is_transient(&error) => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(15 * attempt));
            }
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                return fs::write(path, contents).map_err(|_| error);
            }
        }
    }
}

/// Отказ из-за того, что файл держит другая программа: антивирус, индексатор, редактор.
fn is_transient(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(5 | 32 | 33)) || error.kind() == io::ErrorKind::PermissionDenied
}

/// Скрытые файлы: с точки, как в macOS, и с атрибутом «скрытый» или «системный» в Windows.
pub fn is_hidden(name: &str, metadata: &Metadata) -> bool {
    name.starts_with('.') || has_hidden_attribute(metadata)
}

#[cfg(windows)]
fn has_hidden_attribute(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    metadata.file_attributes() & (HIDDEN | SYSTEM) != 0
}

#[cfg(not(windows))]
fn has_hidden_attribute(_: &Metadata) -> bool {
    false
}

/// Время изменения в секундах от 1970 года; 0 — неизвестно.
pub fn modified_seconds(metadata: &Metadata) -> f64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0.0, |duration| duration.as_secs_f64())
}

/// Абсолютный путь без `.` и `..` и без завершающего разделителя.
pub fn normalize(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    absolute.components().collect()
}

/// Путь для показа и хранения в реестре: обычные обратные слеши, без `\\?\`.
pub fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\").map(str::to_owned).unwrap_or_else(|| text.into_owned())
}

/// Лежит ли путь внутри папки (или совпадает с ней). В Windows регистр в путях не важен.
pub fn is_within(path: &Path, folder: &Path) -> bool {
    let path = normalize(path);
    let folder = normalize(folder);
    let mut inner = path.components();
    folder.components().all(|component| {
        inner.next().is_some_and(|next| {
            if cfg!(windows) {
                next.as_os_str().to_string_lossy().to_lowercase() == component.as_os_str().to_string_lossy().to_lowercase()
            } else {
                next == component
            }
        })
    })
}

/// Путь внутри корня по сегментам страницы `A/B/C`.
pub fn join_segments(root: &Path, relative: &str) -> PathBuf {
    let mut result = root.to_path_buf();
    for segment in relative.split('/').filter(|segment| !segment.is_empty()) {
        result.push(segment);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_file() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("index.md");
        write_atomic(&file, "первый".as_bytes()).unwrap();
        write_atomic(&file, "второй".as_bytes()).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "второй");
        assert_eq!(fs::read_dir(folder.path()).unwrap().count(), 1);
    }

    #[test]
    fn containment_ignores_case_on_windows() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("Работа");
        assert!(is_within(&root.join("Страница"), &root));
        assert!(is_within(&root, &root));
        assert!(!is_within(&folder.path().join("Работа 2"), &root));
        if cfg!(windows) {
            let upper = PathBuf::from(root.to_string_lossy().to_uppercase());
            assert!(is_within(&upper.join("x"), &root));
        }
    }
}
