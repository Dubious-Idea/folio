//! Имена вложений, которые не ломают Markdown-ссылки и не затирают соседние файлы.

use std::path::Path;

use chrono::{DateTime, Local};

const MAXIMUM_STEM_LENGTH: usize = 80;
/// Такие имена даёт буфер обмена при вставке снимка экрана.
const GENERIC_STEMS: [&str; 6] = ["image", "Image", "изображение", "Вставка", "Снимок", "Снимок экрана"];

fn is_unsafe(character: char) -> bool {
    " ()[]{}<>#%?*:|\"'\\/".contains(character) || character.is_whitespace() || character.is_control()
}

pub fn available_name(preferred: &str, folder: &Path, now: DateTime<Local>) -> String {
    let preferred_path = Path::new(preferred);
    let extension = preferred_path.extension().map(|extension| extension.to_string_lossy().to_lowercase()).unwrap_or_default();
    let original_stem = preferred_path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
    let fallback = pasted_stem(now);
    let stem = clean_stem(if GENERIC_STEMS.contains(&original_stem.as_str()) { "" } else { &original_stem }, &fallback);
    let suffix = if extension.is_empty() { String::new() } else { format!(".{extension}") };

    let mut candidate = format!("{stem}{suffix}");
    let mut counter = 2;
    while folder.join(&candidate).exists() {
        candidate = format!("{stem}-{counter}{suffix}");
        counter += 1;
    }
    candidate
}

pub fn clean_stem(stem: &str, fallback: &str) -> String {
    let replaced: String = stem.chars().map(|character| if is_unsafe(character) { '-' } else { character }).collect();
    let collapsed = replaced.split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-");
    let trimmed = collapsed.trim_matches(['.', '-', '_']);
    let limited: String = trimmed.chars().take(MAXIMUM_STEM_LENGTH).collect();
    if limited.is_empty() { fallback.to_owned() } else { limited }
}

fn pasted_stem(date: DateTime<Local>) -> String {
    format!("Вставка-{}", date.format("%Y-%m-%d-%H%M%S"))
}

/// Картинка ли это — по расширению, как `UTType.image`.
pub fn is_image(file_name: &str) -> bool {
    let extension = Path::new(file_name).extension().map(|extension| extension.to_string_lossy().to_lowercase()).unwrap_or_default();
    matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "ico" | "tif" | "tiff" | "heic" | "heif" | "avif" | "jxl"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn cleans_name() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(available_name("Схема процесса (финал).PNG", folder.path(), Local::now()), "Схема-процесса-финал.png");
    }

    #[test]
    fn avoids_collisions() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("ТЗ.pdf"), "").unwrap();
        std::fs::write(folder.path().join("ТЗ-2.pdf"), "").unwrap();
        assert_eq!(available_name("ТЗ.pdf", folder.path(), Local::now()), "ТЗ-3.pdf");
    }

    #[test]
    fn names_pasted_images() {
        let folder = tempfile::tempdir().unwrap();
        let date = Local.with_ymd_and_hms(2026, 9, 15, 20, 41, 3).unwrap();
        let name = available_name("image.png", folder.path(), date);
        assert_eq!(name, "Вставка-2026-09-15-204103.png");
    }
}
