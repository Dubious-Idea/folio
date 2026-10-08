//! Стартовое содержимое нового пространства. Тексты в `seed/` извлечены из `SpaceContent.swift`
//! без изменений, поэтому пространства из Windows и macOS выглядят одинаково.

use chrono::{DateTime, Local};

use crate::bootstrap::SpaceKind;
use crate::frontmatter::FrontmatterWriter;
use crate::text::{iso_date, make_page_id};

pub struct SeedFile {
    pub path: &'static str,
    pub contents: String,
}

const HOME_WORK: &str = include_str!("seed/home-work.md");
const HOME_PERSONAL: &str = include_str!("seed/home-personal.md");
const INBOX_README: &str = include_str!("seed/inbox-readme.md");
const GUIDE: &str = include_str!("seed/guide.md");
const GUIDE_WORK: &str = include_str!("seed/guide-work.md");
const TEMPLATE_MEETING: &str = include_str!("seed/template-meeting.md");
const TEMPLATE_REQUIREMENTS: &str = include_str!("seed/template-requirements.md");
const TEMPLATE_SPECIFICATION: &str = include_str!("seed/template-specification.md");
const TEMPLATE_NOTE: &str = include_str!("seed/template-note.md");
const REFERENCE: &str = include_str!("seed/reference.md");

/// Кроме `.DS_Store` с macOS, в историю не попадают эскизы и настройки папок Проводника.
const GITIGNORE: &str = ".DS_Store\nThumbs.db\ndesktop.ini\n";

pub fn files(kind: SpaceKind, space_name: &str, now: DateTime<Local>) -> Vec<SeedFile> {
    let mut files = vec![
        SeedFile { path: ".gitignore", contents: GITIGNORE.to_owned() },
        SeedFile { path: "CLAUDE.md", contents: guide(space_name, kind) },
        SeedFile { path: "index.md", contents: page(space_name, now, &[], home_body(kind)) },
        SeedFile { path: "_Шаблоны/Страница.md", contents: String::new() },
        SeedFile { path: "_Входящие/README.md", contents: INBOX_README.to_owned() },
    ];
    let reference = SeedFile {
        path: "Справка по разметке/index.md",
        contents: page(
            "Справка по разметке",
            now,
            &["status: Готово", "labels: [справка]", "order: 1000"],
            &REFERENCE.replace("{{space_name}}", space_name),
        ),
    };
    match kind {
        SpaceKind::Work => files.extend([
            SeedFile { path: "_Шаблоны/Протокол встречи.md", contents: TEMPLATE_MEETING.to_owned() },
            SeedFile { path: "_Шаблоны/Требования.md", contents: TEMPLATE_REQUIREMENTS.to_owned() },
            SeedFile { path: "_Шаблоны/Техническое задание.md", contents: TEMPLATE_SPECIFICATION.to_owned() },
            reference,
        ]),
        SpaceKind::Personal => files.extend([SeedFile { path: "_Шаблоны/Заметка.md", contents: TEMPLATE_NOTE.to_owned() }, reference]),
        SpaceKind::Blank => {}
    }
    files
}

fn page(title: &str, now: DateTime<Local>, extra: &[&str], body: &str) -> String {
    let mut lines = vec![
        format!("id: {}", make_page_id()),
        format!("title: {}", FrontmatterWriter::scalar(title)),
        format!("created: {}", iso_date(now)),
    ];
    lines.extend(extra.iter().map(|line| (*line).to_owned()));
    FrontmatterWriter::document(&lines, body)
}

fn home_body(kind: SpaceKind) -> &'static str {
    match kind {
        SpaceKind::Work => HOME_WORK,
        SpaceKind::Personal => HOME_PERSONAL,
        SpaceKind::Blank => "[children]",
    }
}

fn guide(space_name: &str, kind: SpaceKind) -> String {
    let work_rules = if kind == SpaceKind::Work { GUIDE_WORK } else { "" };
    GUIDE.replace("{{space_name}}", space_name).replace("{{work_rules}}", work_rules)
}
