use std::ops::Range;

use uuid::Uuid;

use crate::space::PageRef;
use crate::text::title_key;

/// Ссылка `[[Пространство::Заголовок|текст]]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WikiLink {
    pub space: Option<String>,
    pub title: String,
    pub label: Option<String>,
}

impl WikiLink {
    pub fn key(&self) -> String {
        Self::make_key(self.space.as_deref(), &self.title)
    }

    pub fn markdown(&self) -> String {
        let mut result = String::from("[[");
        if let Some(space) = &self.space {
            result.push_str(space);
            result.push_str("::");
        }
        result.push_str(&self.title);
        if let Some(label) = &self.label {
            result.push('|');
            result.push_str(label);
        }
        result.push_str("]]");
        result
    }

    /// Совпадает с `wikiLinkKey` в веб-части.
    pub fn make_key(space: Option<&str>, title: &str) -> String {
        format!("{}::{}", title_key(space.unwrap_or("")), title_key(title))
    }
}

pub struct WikiLinkOccurrence {
    pub link: WikiLink,
    pub range: Range<usize>,
}

pub struct WikiLinkParser;

impl WikiLinkParser {
    pub fn links(markdown: &str) -> Vec<WikiLink> {
        Self::occurrences(markdown).into_iter().map(|occurrence| occurrence.link).collect()
    }

    pub fn occurrences(markdown: &str) -> Vec<WikiLinkOccurrence> {
        if !markdown.contains("[[") {
            return Vec::new();
        }
        let code = MarkdownCode::ranges(markdown);
        let mut result = Vec::new();
        let mut cursor = 0;
        while let Some(found) = markdown[cursor..].find("[[") {
            let start = cursor + found;
            match match_wiki_link(markdown, start) {
                Some((range, inner, label)) => {
                    cursor = range.end;
                    if code.iter().any(|code| overlaps(code, &range)) {
                        continue;
                    }
                    let label = label.map(str::trim).filter(|label| !label.is_empty()).map(str::to_owned);
                    if let Some(link) = parse_target(inner, label) {
                        result.push(WikiLinkOccurrence { link, range });
                    }
                }
                None => cursor = start + 1,
            }
        }
        result
    }

    /// Переписывает заголовок в подходящих ссылках; `None`, если менять нечего.
    pub fn rewrite(markdown: &str, new_title: &str, should_rename: impl Fn(&WikiLink) -> bool) -> Option<String> {
        let targets: Vec<WikiLinkOccurrence> =
            Self::occurrences(markdown).into_iter().filter(|occurrence| should_rename(&occurrence.link)).collect();
        if targets.is_empty() {
            return None;
        }
        let mut result = String::with_capacity(markdown.len() + targets.len() * new_title.len());
        let mut cursor = 0;
        for occurrence in targets {
            result.push_str(&markdown[cursor..occurrence.range.start]);
            let renamed = WikiLink { title: new_title.to_owned(), ..occurrence.link };
            result.push_str(&renamed.markdown());
            cursor = occurrence.range.end;
        }
        result.push_str(&markdown[cursor..]);
        Some(result)
    }
}

/// `\[\[([^\[\]|\n]+?)(?:\|([^\[\]\n]+?))?\]\]` без движка регулярных выражений.
fn match_wiki_link(text: &str, start: usize) -> Option<(Range<usize>, &str, Option<&str>)> {
    let bytes = text.as_bytes();
    let inner_start = start + 2;
    let inner_end = inner_start + bytes[inner_start..].iter().position(|byte| matches!(byte, b'[' | b']' | b'|' | b'\n'))?;
    if inner_end == inner_start {
        return None;
    }
    match bytes[inner_end] {
        b']' if bytes.get(inner_end + 1) == Some(&b']') => Some((start..inner_end + 2, &text[inner_start..inner_end], None)),
        b'|' => {
            let label_start = inner_end + 1;
            let label_end = label_start + bytes[label_start..].iter().position(|byte| matches!(byte, b'[' | b']' | b'\n'))?;
            let closes = bytes[label_end] == b']' && bytes.get(label_end + 1) == Some(&b']');
            (closes && label_end > label_start)
                .then(|| (start..label_end + 2, &text[inner_start..inner_end], Some(&text[label_start..label_end])))
        }
        _ => None,
    }
}

fn parse_target(inner: &str, label: Option<String>) -> Option<WikiLink> {
    let (space, title) = match inner.split_once("::") {
        Some((space, title)) => (Some(space.trim()).filter(|space| !space.is_empty()).map(str::to_owned), title.trim()),
        None => (None, inner.trim()),
    };
    (!title.is_empty()).then(|| WikiLink { space, title: title.to_owned(), label })
}

fn overlaps(lhs: &Range<usize>, rhs: &Range<usize>) -> bool {
    lhs.start < rhs.end && rhs.start < lhs.end
}

/// Участки кода, внутри которых разметку ссылок не трогаем.
pub struct MarkdownCode;

impl MarkdownCode {
    pub fn ranges(markdown: &str) -> Vec<Range<usize>> {
        let mut ranges = fenced_ranges(markdown);
        let fenced_count = ranges.len();
        let inline: Vec<Range<usize>> = inline_ranges(markdown)
            .into_iter()
            .filter(|range| !ranges[..fenced_count].iter().any(|fenced| overlaps(fenced, range)))
            .collect();
        ranges.extend(inline);
        ranges
    }
}

fn fenced_ranges(markdown: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut open: Option<(u8, usize, usize)> = None;
    let mut line_start = 0;
    while line_start < markdown.len() {
        let line_end = markdown[line_start..].find('\n').map_or(markdown.len(), |index| line_start + index);
        let line = markdown[line_start..line_end].strip_suffix('\r').unwrap_or(&markdown[line_start..line_end]);
        let content_end = line_start + line.len();
        let line = line.trim_start_matches(' ');
        if let Some(&marker) = line.as_bytes().first().filter(|&&first| first == b'`' || first == b'~') {
            let length = line.bytes().take_while(|&byte| byte == marker).count();
            if length >= 3 {
                match open {
                    Some((open_marker, open_length, open_start)) => {
                        let is_closing =
                            marker == open_marker && length >= open_length && line[length..].chars().all(char::is_whitespace);
                        if is_closing {
                            ranges.push(open_start..content_end);
                            open = None;
                        }
                    }
                    None => open = Some((marker, length, line_start)),
                }
            }
        }
        line_start = line_end + 1;
    }
    if let Some((_, _, open_start)) = open {
        ranges.push(open_start..markdown.len());
    }
    ranges
}

/// `(`+)[^`\n]+?\1`: строка кода в обратных кавычках.
fn inline_ranges(markdown: &str) -> Vec<Range<usize>> {
    let bytes = markdown.as_bytes();
    let mut ranges = Vec::new();
    let mut index = 0;
    while let Some(found) = bytes[index..].iter().position(|&byte| byte == b'`') {
        let start = index + found;
        let run = bytes[start..].iter().take_while(|&&byte| byte == b'`').count();
        let content_start = start + run;
        let content_end = content_start + bytes[content_start..].iter().position(|&byte| byte == b'`' || byte == b'\n').unwrap_or(bytes.len() - content_start);
        let closes = content_end > content_start
            && content_end + run <= bytes.len()
            && bytes[content_end..content_end + run].iter().all(|&byte| byte == b'`');
        if closes {
            ranges.push(start..content_end + run);
            index = content_end + run;
        } else {
            index = start + 1;
        }
    }
    ranges
}

/// Относительные ссылки на файлы рядом со страницей: картинки и вложения.
pub struct AttachmentReferences;

impl AttachmentReferences {
    pub fn relative_paths(markdown: &str) -> Vec<String> {
        if !markdown.contains("](") {
            return Vec::new();
        }
        let code = MarkdownCode::ranges(markdown);
        let bytes = markdown.as_bytes();
        let mut result = Vec::new();
        let mut index = 0;
        while let Some(found) = bytes[index..].iter().position(|&byte| byte == b'[') {
            let start = index + found;
            let Some((end, target)) = match_link_target(markdown, start) else {
                index = start + 1;
                continue;
            };
            index = end;
            let range = if start > 0 && bytes[start - 1] == b'!' { start - 1..end } else { start..end };
            if code.iter().any(|code| overlaps(code, &range)) {
                continue;
            }
            let target = target.strip_prefix('<').and_then(|inner| inner.strip_suffix('>')).unwrap_or(target);
            let is_external = target.contains(':') || target.starts_with('#') || target.starts_with('/');
            if is_external {
                continue;
            }
            let without_fragment = target.split('#').next().unwrap_or(target);
            result.push(percent_decode(without_fragment).unwrap_or_else(|| without_fragment.to_owned()));
        }
        result
    }
}

/// `\[[^\]\n]*\]\((<[^>\n]+>|[^)\s]+)(?:\s+"[^"\n]*")?\)` начиная с `[`.
fn match_link_target(text: &str, start: usize) -> Option<(usize, &str)> {
    let bytes = text.as_bytes();
    let label_end = start + 1 + bytes[start + 1..].iter().position(|&byte| byte == b']' || byte == b'\n')?;
    if bytes[label_end] != b']' || bytes.get(label_end + 1) != Some(&b'(') {
        return None;
    }
    let target_start = label_end + 2;
    let target_end = if bytes.get(target_start) == Some(&b'<') {
        let close = target_start + 1 + bytes[target_start + 1..].iter().position(|&byte| byte == b'>' || byte == b'\n')?;
        if bytes[close] != b'>' || close == target_start + 1 {
            return None;
        }
        close + 1
    } else {
        let length = text[target_start..].find(|character: char| character == ')' || character.is_whitespace()).unwrap_or(text.len() - target_start);
        if length == 0 {
            return None;
        }
        target_start + length
    };
    let target = &text[target_start..target_end];
    if bytes.get(target_end) == Some(&b')') {
        return Some((target_end + 1, target));
    }
    // Необязательный заголовок ссылки: `[текст](файл "подсказка")`.
    let after_space = target_end + text[target_end..].find(|character: char| !character.is_whitespace())?;
    if after_space == target_end || bytes[after_space] != b'"' {
        return None;
    }
    let title_end = after_space + 1 + bytes[after_space + 1..].iter().position(|&byte| byte == b'"' || byte == b'\n')?;
    (bytes[title_end] == b'"' && bytes.get(title_end + 1) == Some(&b')')).then_some((title_end + 2, target))
}

pub(crate) fn percent_decode(text: &str) -> Option<String> {
    if !text.contains('%') {
        return Some(text.to_owned());
    }
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = text.get(index + 1..index + 3)?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

pub(crate) fn percent_encode(text: &str, keep_slash: bool) -> String {
    let mut result = String::with_capacity(text.len() * 3);
    for &byte in text.as_bytes() {
        let unreserved = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') || (keep_slash && byte == b'/');
        if unreserved {
            result.push(byte as char);
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    result
}

/// Системная ссылка `folio://open?space=<id>&path=<путь>[&edit=1]`: открывает страницу в приложении.
pub struct PageLink;

impl PageLink {
    pub const SCHEME: &'static str = "folio";
    pub const OPEN_HOST: &'static str = "open";

    pub fn url(page: &PageRef, editing: bool) -> String {
        let mut url = format!(
            "{}://{}?space={}&path={}",
            Self::SCHEME,
            Self::OPEN_HOST,
            crate::space::uuid_string(page.space_id),
            percent_encode(&page.path, true)
        );
        if editing {
            url.push_str("&edit=1");
        }
        url
    }

    /// Страница и просьба открыть её на правку; `None` — это не ссылка Folio на страницу.
    pub fn parse(url: &str) -> Option<(PageRef, bool)> {
        let (scheme, rest) = url.trim().split_once("://")?;
        if !scheme.eq_ignore_ascii_case(Self::SCHEME) {
            return None;
        }
        let (host, query) = rest.split_once('?')?;
        if !host.trim_end_matches('/').eq_ignore_ascii_case(Self::OPEN_HOST) {
            return None;
        }
        let mut space = None;
        let mut path = String::new();
        let mut editing = false;
        for pair in query.split('&') {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            let value = percent_decode(value)?;
            match name {
                "space" => space = Uuid::parse_str(&value).ok(),
                "path" => path = value,
                "edit" => editing = value == "1",
                _ => {}
            }
        }
        Some((PageRef { space_id: space?, path }, editing))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(space: Option<&str>, title: &str, label: Option<&str>) -> WikiLink {
        WikiLink { space: space.map(str::to_owned), title: title.to_owned(), label: label.map(str::to_owned) }
    }

    #[test]
    fn parses_forms() {
        let links = WikiLinkParser::links("[[Протокол]] и [[Личное::Идеи|мои идеи]], [[Встреча: заказчик]]");
        assert_eq!(
            links,
            [link(None, "Протокол", None), link(Some("Личное"), "Идеи", Some("мои идеи")), link(None, "Встреча: заказчик", None)]
        );
    }

    #[test]
    fn skips_code() {
        let markdown = "Текст [[Живая]]\n\n```\n[[В блоке кода]]\n```\n\nИ `[[в строке кода]]`";
        let titles: Vec<String> = WikiLinkParser::links(markdown).into_iter().map(|link| link.title).collect();
        assert_eq!(titles, ["Живая"]);
    }

    #[test]
    fn unclosed_fence_runs_to_end() {
        let titles: Vec<String> = WikiLinkParser::links("[[До]]\n~~~\n[[После]]").into_iter().map(|link| link.title).collect();
        assert_eq!(titles, ["До"]);
    }

    #[test]
    fn nested_brackets_find_inner_link() {
        let titles: Vec<String> = WikiLinkParser::links("[[[Страница]] [[a]b]]").into_iter().map(|link| link.title).collect();
        assert_eq!(titles, ["Страница"]);
    }

    #[test]
    fn rewrite_preserves_label() {
        let markdown = "[[Старое]], [[старое|текст]], [[Личное::Старое]], [[Другое]]";
        let result = WikiLinkParser::rewrite(markdown, "Новое", |link| title_key(&link.title) == "старое" && link.space.is_none());
        assert_eq!(result.as_deref(), Some("[[Новое]], [[Новое|текст]], [[Личное::Старое]], [[Другое]]"));
        assert_eq!(WikiLinkParser::rewrite(markdown, "Новое", |_| false), None);
    }

    #[test]
    fn key_matches_web() {
        assert_eq!(WikiLink::make_key(None, "  Протокол   Встречи "), "::протокол встречи");
        assert_eq!(WikiLink::make_key(Some("Личное"), "Идеи"), "личное::идеи");
    }

    #[test]
    fn page_link_round_trip() {
        let page = PageRef { space_id: Uuid::new_v4(), path: "Проекты/Встреча с заказчиком 15.09 & итоги".to_owned() };
        let url = PageLink::url(&page, false);
        assert_eq!(PageLink::parse(&url), Some((page.clone(), false)));
        assert_eq!(PageLink::parse(&PageLink::url(&page, true)), Some((page, true)));
        assert_eq!(PageLink::parse("folio://app/index.html"), None);
    }

    #[test]
    fn attachments() {
        let markdown = "![Схема](схема%201.png) [ТЗ](<ТЗ финал.pdf>) ![](https://example.com/a.png) [[Страница]] [якорь](#раздел) [с подсказкой](файл.txt \"текст\")";
        assert_eq!(AttachmentReferences::relative_paths(markdown), ["схема 1.png", "ТЗ финал.pdf", "файл.txt"]);
    }

    #[test]
    fn inline_code_with_double_backticks() {
        assert_eq!(inline_ranges("``a`"), [1..4]);
        assert_eq!(inline_ranges("x ``co`de`` y"), [3..7]);
    }
}
