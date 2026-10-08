use std::cmp::Ordering;

use chrono::{DateTime, Local};

use crate::error::{FolioError, Result};

/// Схлопывает пробельные символы в один пробел и обрезает края.
pub fn collapse_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for word in text.split_whitespace() {
        if !result.is_empty() {
            result.push(' ');
        }
        result.push_str(word);
    }
    result
}

/// Ключ для сравнения заголовков: без учёта регистра и лишних пробелов.
/// Веб-часть строит ключ ссылки по тому же правилу (`wikiLinkKey`).
pub fn title_key(text: &str) -> String {
    collapse_whitespace(text).to_lowercase()
}

pub const MAXIMUM_TITLE_LENGTH: usize = 200;

/// Возвращает заголовок без лишних пробелов или объясняет, почему он не подходит.
pub fn validate_title(title: &str) -> Result<String> {
    let invalid = |reason: &str| FolioError::InvalidTitle { title: title.to_owned(), reason: reason.to_owned() };
    let cleaned = collapse_whitespace(title);
    if cleaned.is_empty() {
        return Err(invalid("заголовок пустой"));
    }
    if cleaned.contains(['[', ']', '|']) {
        return Err(invalid("символы [ ] | ломают ссылки на страницу"));
    }
    if cleaned.contains("::") {
        return Err(invalid("«::» в ссылках отделяет пространство от страницы"));
    }
    if cleaned.chars().count() > MAXIMUM_TITLE_LENGTH {
        return Err(invalid(&format!("длиннее {MAXIMUM_TITLE_LENGTH} символов")));
    }
    Ok(cleaned)
}

/// Пути страниц внутри пространства: всегда через `/`, как в git и в версии для macOS.
pub mod page_path {
    pub fn join(parent: &str, name: &str) -> String {
        if parent.is_empty() { name.to_owned() } else { format!("{parent}/{name}") }
    }

    pub fn parent(path: &str) -> Option<&str> {
        if path.is_empty() {
            return None;
        }
        Some(path.rfind('/').map_or("", |slash| &path[..slash]))
    }

    pub fn name(path: &str) -> &str {
        path.rsplit('/').find(|part| !part.is_empty()).unwrap_or("")
    }

    /// Файл страницы относительно корня пространства — так его видит git.
    pub fn page_file(path: &str) -> String {
        if path.is_empty() { super::PAGE_FILE_NAME.to_owned() } else { format!("{path}/{}", super::PAGE_FILE_NAME) }
    }
}

pub const PAGE_FILE_NAME: &str = "index.md";

/// Короткий id страницы для шапки: 12 шестнадцатеричных знаков.
pub fn make_page_id() -> String {
    let mut id = uuid::Uuid::new_v4().simple().to_string();
    id.truncate(12);
    id
}

pub fn iso_date(date: DateTime<Local>) -> String {
    date.format("%Y-%m-%d").to_string()
}

pub fn russian_date(date: DateTime<Local>) -> String {
    date.format("%d.%m.%Y").to_string()
}

/// «15.09.2026, 20:41» — как короткая дата и время в русской локали macOS.
pub fn russian_date_time(date: DateTime<Local>) -> String {
    date.format("%d.%m.%Y, %H:%M").to_string()
}

const GENITIVE_MONTHS: [&str; 12] = [
    "января", "февраля", "марта", "апреля", "мая", "июня",
    "июля", "августа", "сентября", "октября", "ноября", "декабря",
];

/// «15 сентября 2026 г. в 20:41» — длинная дата для шапки страницы.
pub fn russian_long_date_time(date: DateTime<Local>) -> String {
    use chrono::Datelike;
    let month = GENITIVE_MONTHS[date.month0() as usize];
    format!("{} {month} {} г. в {}", date.day(), date.year(), date.format("%H:%M"))
}

/// Порядок как в Проводнике и Finder: без учёта регистра, числа сравниваются по значению.
pub fn natural_cmp(lhs: &str, rhs: &str) -> Ordering {
    let mut left = lhs.chars().peekable();
    let mut right = rhs.chars().peekable();
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return lhs.cmp(rhs),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(a), Some(b)) if a.is_ascii_digit() && b.is_ascii_digit() => {
                let first = take_number(&mut left);
                let second = take_number(&mut right);
                let ordering = compare_digits(&first, &second);
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            (Some(a), Some(b)) => {
                let ordering = fold(a).cmp(&fold(b));
                if ordering != Ordering::Equal {
                    return ordering;
                }
                left.next();
                right.next();
            }
        }
    }
}

fn take_number(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut digits = String::new();
    while let Some(&character) = chars.peek() {
        if !character.is_ascii_digit() {
            break;
        }
        digits.push(character);
        chars.next();
    }
    digits
}

fn compare_digits(lhs: &str, rhs: &str) -> Ordering {
    let left = lhs.trim_start_matches('0');
    let right = rhs.trim_start_matches('0');
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

/// Буква для сравнения: строчная, «ё» стоит рядом с «е», а не после «я».
fn fold(character: char) -> (u32, u32) {
    let lower = character.to_lowercase().next().unwrap_or(character);
    match lower {
        'ё' => ('е' as u32, 1),
        other => (other as u32, 0),
    }
}

pub(crate) trait NonEmpty {
    fn non_empty(self) -> Option<String>;
}

impl NonEmpty for String {
    fn non_empty(self) -> Option<String> {
        if self.is_empty() { None } else { Some(self) }
    }
}

impl NonEmpty for &str {
    fn non_empty(self) -> Option<String> {
        if self.is_empty() { None } else { Some(self.to_owned()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut names = vec!["Страница 10", "страница 2", "Ёлка", "Елена", "Яблоко", "Альфа"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["Альфа", "Елена", "Ёлка", "страница 2", "Страница 10", "Яблоко"]);
    }

    #[test]
    fn page_paths() {
        assert_eq!(page_path::parent("A/B/C"), Some("A/B"));
        assert_eq!(page_path::parent("A"), Some(""));
        assert_eq!(page_path::parent(""), None);
        assert_eq!(page_path::name("A/B/C"), "C");
        assert_eq!(page_path::join("", "A"), "A");
        assert_eq!(page_path::page_file("A/B"), "A/B/index.md");
    }
}
