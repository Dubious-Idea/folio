//! Запись правок из приложения: тело и свойства страницы меняются, остальная шапка остаётся как была.

use crate::frontmatter::{FrontmatterWriter, PageDocument};
use crate::text::{collapse_whitespace, make_page_id};

/// Текст файла с прежней шапкой и новым телом.
pub fn replacing_body(text: &str, body: &str) -> String {
    let document = PageDocument::parse(text);
    let content = body.trim();
    match document.raw_frontmatter {
        None if content.is_empty() => String::new(),
        None => format!("{content}\n"),
        Some(raw) => FrontmatterWriter::document(&lines(&raw), content),
    }
}

/// Меняет статус и метки; пустой статус и пустой список меток убирают поля из шапки.
/// Свой цвет статуса сбрасывается, только если сменился сам статус.
pub fn setting_properties(text: &str, status: Option<&str>, labels: &[String]) -> String {
    let document = PageDocument::parse(text);
    let clean_status = status.map(collapse_whitespace).filter(|status| !status.is_empty());
    let clean_labels: Vec<String> = labels.iter().map(|label| collapse_whitespace(label)).filter(|label| !label.is_empty()).collect();

    let mut fields = document.raw_frontmatter.as_deref().map_or_else(|| vec![format!("id: {}", make_page_id())], lines);
    fields = setting("status", clean_status.as_deref().map(FrontmatterWriter::scalar), &fields);
    if clean_status.as_deref() != document.frontmatter.status.as_ref().map(|status| status.label.as_str()) {
        fields = setting("status_color", None, &fields);
    }
    let labels_value = (!clean_labels.is_empty()).then(|| {
        let items: Vec<String> = clean_labels.iter().map(|label| FrontmatterWriter::flow_scalar(label)).collect();
        format!("[{}]", items.join(", "))
    });
    fields = setting("labels", labels_value, &fields);
    FrontmatterWriter::document(&fields, &document.body)
}

fn lines(raw: &str) -> Vec<String> {
    if raw.is_empty() { Vec::new() } else { raw.split('\n').map(str::to_owned).collect() }
}

fn setting(key: &str, value: Option<String>, fields: &[String]) -> Vec<String> {
    let yaml = fields.join("\n");
    match value {
        None => FrontmatterWriter::lines_excluding(&yaml, &[key]),
        Some(value) => FrontmatterWriter::replacing_field(key, &value, &yaml),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::{PageStatus, StatusColor};

    const PAGE: &str = "---\nid: abc123\ntitle: Протокол\n# комментарий остаётся\ncustom: значение\nstatus: В работе\nstatus_color: red\n---\n\nСтарый текст";

    #[test]
    fn replaces_body_keeping_frontmatter() {
        let result = replacing_body(PAGE, "\n# Новый текст\n\n- пункт\n\n");
        assert_eq!(
            result,
            "---\nid: abc123\ntitle: Протокол\n# комментарий остаётся\ncustom: значение\nstatus: В работе\nstatus_color: red\n---\n\n# Новый текст\n\n- пункт\n"
        );
    }

    #[test]
    fn body_without_frontmatter() {
        assert_eq!(replacing_body("старое", "новое"), "новое\n");
        assert_eq!(replacing_body("старое", "  "), "");
    }

    #[test]
    fn sets_properties() {
        let result = setting_properties(PAGE, Some("На согласовании"), &["встреча".into(), "a, b".into()]);
        let document = PageDocument::parse(&result);
        assert_eq!(document.frontmatter.status, Some(PageStatus::new("На согласовании", None)));
        assert_eq!(document.frontmatter.status.as_ref().map(|status| status.color), Some(StatusColor::Yellow));
        assert_eq!(document.frontmatter.labels, ["встреча", "a, b"]);
        let raw = document.raw_frontmatter.unwrap();
        assert!(raw.contains("custom: значение"));
        assert!(!raw.contains("status_color"));
        assert_eq!(document.body, "Старый текст\n");
    }

    #[test]
    fn keeps_color_and_removes_empty() {
        let same = PageDocument::parse(&setting_properties(PAGE, Some("В работе"), &[]));
        assert_eq!(same.frontmatter.status, Some(PageStatus::new("В работе", Some(StatusColor::Red))));

        let cleared = PageDocument::parse(&setting_properties(PAGE, None, &[]));
        assert_eq!(cleared.frontmatter.status, None);
        assert!(!cleared.raw_frontmatter.unwrap().contains("status"));
        assert_eq!(cleared.frontmatter.title.as_deref(), Some("Протокол"));
    }
}
