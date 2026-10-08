use serde::Serialize;
use yaml_rust2::{Yaml, YamlLoader};

use crate::text::{NonEmpty, collapse_whitespace, title_key};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StatusColor {
    Grey,
    Blue,
    Green,
    Yellow,
    Red,
    Purple,
}

impl StatusColor {
    pub fn as_str(self) -> &'static str {
        match self {
            StatusColor::Grey => "grey",
            StatusColor::Blue => "blue",
            StatusColor::Green => "green",
            StatusColor::Yellow => "yellow",
            StatusColor::Red => "red",
            StatusColor::Purple => "purple",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "grey" => StatusColor::Grey,
            "blue" => StatusColor::Blue,
            "green" => StatusColor::Green,
            "yellow" => StatusColor::Yellow,
            "red" => StatusColor::Red,
            "purple" => StatusColor::Purple,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct PageStatus {
    pub label: String,
    pub color: StatusColor,
}

impl PageStatus {
    pub fn new(label: impl Into<String>, color: Option<StatusColor>) -> Self {
        let label = label.into();
        let color = color.unwrap_or_else(|| Self::default_color(&label));
        PageStatus { label, color }
    }

    pub fn default_color(label: &str) -> StatusColor {
        match title_key(label).as_str() {
            "в работе" | "в процессе" => StatusColor::Blue,
            "на согласовании" | "на ревью" | "на проверке" | "ожидает" => StatusColor::Yellow,
            "утверждено" | "согласовано" | "готово" | "сделано" => StatusColor::Green,
            "отклонено" | "заблокировано" | "отменено" => StatusColor::Red,
            "архив" | "устарело" => StatusColor::Purple,
            _ => StatusColor::Grey,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageFrontmatter {
    pub id: Option<String>,
    pub title: Option<String>,
    pub status: Option<PageStatus>,
    pub labels: Vec<String>,
    pub order: Option<f64>,
    pub created: Option<String>,
}

/// Файл страницы: YAML-шапка между строками `---` и Markdown-тело.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageDocument {
    pub frontmatter: PageFrontmatter,
    /// Текст шапки без разделителей; `None`, если шапки нет.
    pub raw_frontmatter: Option<String>,
    pub body: String,
    pub frontmatter_error: Option<String>,
}

impl PageDocument {
    pub fn parse(text: &str) -> PageDocument {
        let unified;
        let mut source = text;
        if source.contains('\r') {
            unified = source.replace("\r\n", "\n");
            source = &unified;
        }
        let source = source.strip_prefix('\u{FEFF}').unwrap_or(source);

        let Some((yaml, body)) = split_frontmatter(source) else {
            return PageDocument { body: source.to_owned(), ..PageDocument::default() };
        };
        match decode_fields(yaml) {
            Ok(frontmatter) => PageDocument {
                frontmatter,
                raw_frontmatter: Some(yaml.to_owned()),
                body: body.to_owned(),
                frontmatter_error: None,
            },
            Err(error) => PageDocument {
                frontmatter: PageFrontmatter::default(),
                raw_frontmatter: Some(yaml.to_owned()),
                body: body.to_owned(),
                frontmatter_error: Some(error),
            },
        }
    }

    /// Только шапка: для дерева страниц тело не нужно.
    pub fn parse_frontmatter(text: &str) -> Option<PageFrontmatter> {
        let unified;
        let mut source = text;
        if source.contains('\r') {
            unified = source.replace("\r\n", "\n");
            source = &unified;
        }
        let source = source.strip_prefix('\u{FEFF}').unwrap_or(source);
        let (yaml, _) = split_frontmatter(source)?;
        Some(decode_fields(yaml).unwrap_or_default())
    }
}

/// Делит текст на шапку и тело. Шапка начинается строкой `---` и заканчивается `---` или `...`.
pub(crate) fn split_frontmatter(source: &str) -> Option<(&str, &str)> {
    let rest = source.strip_prefix("---\n")?;
    let mut offset = 0;
    loop {
        let line_end = rest[offset..].find('\n').map(|index| offset + index);
        let line = &rest[offset..line_end.unwrap_or(rest.len())];
        let trimmed = line.trim();
        if trimmed == "---" || trimmed == "..." {
            let yaml = if offset == 0 { "" } else { &rest[..offset - 1] };
            let after = line_end.map_or("", |end| &rest[end + 1..]);
            return Some((yaml, skip_blank_lines(after)));
        }
        offset = line_end? + 1;
    }
}

fn skip_blank_lines(text: &str) -> &str {
    let mut offset = 0;
    while offset < text.len() {
        let line_end = text[offset..].find('\n').map_or(text.len(), |index| offset + index);
        if !text[offset..line_end].trim().is_empty() {
            return &text[offset..];
        }
        offset = line_end + 1;
    }
    ""
}

fn decode_fields(yaml: &str) -> Result<PageFrontmatter, String> {
    if yaml.trim().is_empty() {
        return Ok(PageFrontmatter::default());
    }
    let documents = YamlLoader::load_from_str(yaml).map_err(|error| error.to_string())?;
    let Some(Yaml::Hash(fields)) = documents.into_iter().next() else {
        return Err("шапка должна быть набором полей «ключ: значение»".to_owned());
    };
    let field = |key: &str| fields.get(&Yaml::String(key.to_owned()));

    let status_color = field("status_color").and_then(scalar_string).and_then(|value| StatusColor::parse(&value));
    Ok(PageFrontmatter {
        id: field("id").and_then(scalar_string),
        title: field("title").and_then(scalar_string).map(|title| collapse_whitespace(&title)),
        status: field("status").and_then(scalar_string).map(|label| PageStatus::new(label, status_color)),
        labels: field("labels").map(string_list).unwrap_or_default(),
        order: field("order").and_then(number),
        created: field("created").and_then(scalar_string),
    })
}

fn scalar_string(value: &Yaml) -> Option<String> {
    match value {
        Yaml::String(text) => text.trim().non_empty(),
        Yaml::Integer(integer) => Some(integer.to_string()),
        Yaml::Real(text) => Some(text.parse::<f64>().map_or_else(|_| text.clone(), |number| format!("{number:?}"))),
        Yaml::Boolean(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn string_list(value: &Yaml) -> Vec<String> {
    let items: Vec<String> = match value {
        Yaml::Array(list) => list.iter().filter_map(scalar_string).collect(),
        Yaml::String(text) => text.split(',').map(|item| item.trim().to_owned()).collect(),
        _ => Vec::new(),
    };
    let mut seen = rustc_hash::FxHashSet::default();
    items.into_iter().filter(|item| !item.is_empty() && seen.insert(title_key(item))).collect()
}

fn number(value: &Yaml) -> Option<f64> {
    match value {
        Yaml::Integer(integer) => Some(*integer as f64),
        Yaml::Real(text) | Yaml::String(text) => text.parse().ok(),
        _ => None,
    }
}

pub struct FrontmatterWriter;

impl FrontmatterWriter {
    pub fn document(frontmatter_lines: &[String], body: &str) -> String {
        let mut result = String::with_capacity(body.len() + 256);
        result.push_str("---\n");
        result.push_str(&frontmatter_lines.join("\n"));
        result.push_str("\n---\n");
        let content = body.trim_matches(is_newline);
        if !content.is_empty() {
            result.push('\n');
            result.push_str(content);
            result.push('\n');
        }
        result
    }

    /// Строка YAML-скаляра: в кавычках только когда без них значение прочитается иначе.
    pub fn scalar(value: &str) -> String {
        const RESERVED: [&str; 8] = ["true", "false", "yes", "no", "on", "off", "null", "~"];
        let needs_quotes = value.is_empty()
            || value.starts_with(|first: char| "-?:,[]{}#&*!|>'\"%@`".contains(first))
            || value.contains(": ")
            || value.contains(" #")
            || value.ends_with(':')
            || value != value.trim()
            || RESERVED.contains(&value.to_lowercase().as_str())
            || value.parse::<f64>().is_ok()
            || looks_like_date(value);
        if needs_quotes { quoted(value) } else { value.to_owned() }
    }

    /// Элемент списка `[a, b]`: кроме обычных случаев, кавычки нужны для запятых и скобок.
    pub fn flow_scalar(value: &str) -> String {
        let plain = Self::scalar(value);
        if plain != value || !value.contains([',', '[', ']', '{', '}']) {
            return plain;
        }
        quoted(value)
    }

    /// Строки шапки верхнего уровня, кроме перечисленных ключей (вместе с их продолжением на следующих строках).
    pub fn lines_excluding(yaml: &str, keys: &[&str]) -> Vec<String> {
        let mut result: Vec<String> = Vec::new();
        let mut is_skipping = false;
        for line in yaml.split('\n') {
            let is_continuation = line.chars().next().is_none_or(|first| first == ' ' || first == '\t' || first == '-');
            if !is_continuation {
                let key = line.split(':').next().unwrap_or("");
                is_skipping = keys.contains(&key);
            }
            if !is_skipping && !(line.is_empty() && result.is_empty()) {
                result.push(line.to_owned());
            }
        }
        while result.last().is_some_and(|line| line.is_empty()) {
            result.pop();
        }
        result
    }

    /// Заменяет поле верхнего уровня; если поля нет — добавляет его после `id`.
    pub fn replacing_field(key: &str, value: &str, yaml: &str) -> Vec<String> {
        let new_line = format!("{key}: {value}");
        let original: Vec<&str> = if yaml.is_empty() { Vec::new() } else { yaml.split('\n').collect() };
        let prefix = format!("{key}:");
        let Some(index) = original.iter().position(|line| line.starts_with(&prefix)) else {
            let insert_at = original.iter().position(|line| line.starts_with("id:")).map_or(0, |index| index + 1);
            let mut result: Vec<String> = original[..insert_at].iter().map(|line| (*line).to_owned()).collect();
            result.push(new_line);
            result.extend(original[insert_at..].iter().map(|line| (*line).to_owned()));
            return result;
        };
        let continuation_end = original[index + 1..]
            .iter()
            .position(|line| !(line.starts_with(' ') || line.starts_with('\t')))
            .map_or(original.len(), |offset| index + 1 + offset);
        let mut result: Vec<String> = original[..index].iter().map(|line| (*line).to_owned()).collect();
        result.push(new_line);
        result.extend(original[continuation_end..].iter().map(|line| (*line).to_owned()));
        result
    }
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// `2026-09-15…` YAML прочитал бы как дату.
fn looks_like_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 10
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
}

fn is_newline(character: char) -> bool {
    matches!(character, '\n' | '\r' | '\u{000B}' | '\u{000C}' | '\u{0085}' | '\u{2028}' | '\u{2029}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_fields() {
        let document = PageDocument::parse(
            "---\nid: abc123\ntitle: Протокол встречи\nstatus: На согласовании\nlabels: [встреча, crm, CRM]\norder: 5\ncreated: 2026-09-15\n---\n\nТекст страницы",
        );
        let frontmatter = &document.frontmatter;
        assert_eq!(frontmatter.id.as_deref(), Some("abc123"));
        assert_eq!(frontmatter.title.as_deref(), Some("Протокол встречи"));
        assert_eq!(frontmatter.status, Some(PageStatus::new("На согласовании", Some(StatusColor::Yellow))));
        assert_eq!(frontmatter.labels, ["встреча", "crm"]);
        assert_eq!(frontmatter.order, Some(5.0));
        assert_eq!(frontmatter.created.as_deref(), Some("2026-09-15"));
        assert_eq!(document.body, "Текст страницы");
    }

    #[test]
    fn explicit_status_color() {
        let document = PageDocument::parse("---\nstatus: В работе\nstatus_color: red\n---\n");
        assert_eq!(document.frontmatter.status, Some(PageStatus::new("В работе", Some(StatusColor::Red))));
    }

    #[test]
    fn labels_from_string() {
        assert_eq!(PageDocument::parse("---\nlabels: встреча, crm\n---\n").frontmatter.labels, ["встреча", "crm"]);
    }

    #[test]
    fn windows_line_endings() {
        let document = PageDocument::parse("---\r\ntitle: Страница\r\n---\r\n\r\nТекст\r\n");
        assert_eq!(document.frontmatter.title.as_deref(), Some("Страница"));
        assert_eq!(document.body, "Текст\n");
    }

    #[test]
    fn invalid_yaml_keeps_body() {
        let document = PageDocument::parse("---\ntitle: [незакрыто\n---\nТекст");
        assert!(document.frontmatter_error.is_some());
        assert_eq!(document.body, "Текст");
    }

    #[test]
    fn no_frontmatter() {
        let document = PageDocument::parse("# Заголовок\n\nТекст");
        assert_eq!(document.raw_frontmatter, None);
        assert_eq!(document.body, "# Заголовок\n\nТекст");
    }

    #[test]
    fn unclosed_frontmatter_is_text() {
        assert_eq!(PageDocument::parse("---\ntitle: Страница\nТекст").raw_frontmatter, None);
    }

    #[test]
    fn scalar_quoting() {
        let cases = [
            ("Протокол встречи", "Протокол встречи"),
            ("Встреча: заказчик", "\"Встреча: заказчик\""),
            ("2026", "\"2026\""),
            ("2026-09-15 созвон", "\"2026-09-15 созвон\""),
            ("yes", "\"yes\""),
            ("Цитата \"клиента\"", "Цитата \"клиента\""),
            ("#тег", "\"#тег\""),
        ];
        for (value, expected) in cases {
            assert_eq!(FrontmatterWriter::scalar(value), expected);
            let parsed = PageDocument::parse(&format!("---\ntitle: {expected}\n---\n"));
            assert_eq!(parsed.frontmatter.title.as_deref(), Some(value), "{value}");
        }
    }

    #[test]
    fn replacing_field_keeps_other_lines() {
        let multiline = "id: abc\ntitle: >\n  Длинный\n  заголовок\nlabels: [a]";
        assert_eq!(FrontmatterWriter::replacing_field("title", "Новый", multiline), ["id: abc", "title: Новый", "labels: [a]"]);
        assert_eq!(
            FrontmatterWriter::replacing_field("title", "Новый", "id: abc\nlabels: [a]"),
            ["id: abc", "title: Новый", "labels: [a]"]
        );
    }

    #[test]
    fn excluding_keys_with_continuation() {
        let yaml = "id: abc\ntitle: Шаблон\nlabels:\n  - встреча\nstatus: Черновик";
        assert_eq!(FrontmatterWriter::lines_excluding(yaml, &["id", "title"]), ["labels:", "  - встреча", "status: Черновик"]);
    }
}
