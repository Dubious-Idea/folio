//! Замер скорости ядра на синтетическом пространстве: `cargo run --release --example bench -- 5000`.

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use chrono::Local;
use folio_core::checker;
use folio_core::frontmatter::FrontmatterWriter;
use folio_core::index::{self, SearchIndex};
use folio_core::operations::PageOperations;
use folio_core::space::{LibrarySnapshot, Space, SpaceManifest, SpaceScanner};

const WORDS: [&str; 16] = [
    "требования", "выгрузка", "заказчик", "интеграция", "протокол", "встреча", "отчёт", "маршрут",
    "склад", "оплата", "договор", "релиз", "задача", "сервер", "клиент", "доставка",
];

fn main() {
    let count: usize = std::env::args().nth(1).and_then(|value| value.parse().ok()).unwrap_or(5000);
    let folder = tempfile::tempdir().expect("временная папка");
    let space = Space::new(SpaceManifest::new("Работа", "briefcase"), &folder.path().join("Работа"));
    space.write_manifest().unwrap();

    let started = Instant::now();
    generate(&space.root, count);
    println!("создание {count} страниц: {:?}", started.elapsed());

    let started = Instant::now();
    let snapshot = SpaceScanner::scan(&space);
    println!("сканирование дерева (первое): {:?} — страниц {}", started.elapsed(), snapshot.pages.len());
    let started = Instant::now();
    let snapshot = Arc::new(SpaceScanner::scan(&space));
    println!("сканирование дерева (повторное): {:?}", started.elapsed());

    let mut index = SearchIndex::open(&folder.path().join("index.sqlite")).unwrap();
    let started = Instant::now();
    let indexed = index.sync(&snapshot).unwrap();
    println!("индексация с нуля: {:?} — документов {indexed}", started.elapsed());
    let started = Instant::now();
    index.sync(&snapshot).unwrap();
    println!("сверка индекса без изменений: {:?}", started.elapsed());

    let path = snapshot.pages.keys().find(|path| !path.is_empty()).unwrap().clone();
    let file = snapshot.file_path(&path);
    let text = fs::read_to_string(&file).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::write(&file, text + "\nНовый абзац про бюджет.\n").unwrap();
    let started = Instant::now();
    let fresh = SpaceScanner::read_page(&snapshot, &path).unwrap();
    let updated = snapshot.with_updated_page(fresh).unwrap();
    index.sync_pages(&updated, std::slice::from_ref(&path)).unwrap();
    println!("правка одной страницы (шапка + индекс): {:?}", started.elapsed());

    for query in ["выгрузк", "договор оплата", "бюджет"] {
        let started = Instant::now();
        let hits = index.search(query, None, 60).unwrap();
        println!("поиск «{query}»: {:?} — найдено {}", started.elapsed(), hits.len());
    }
    let started = Instant::now();
    let hits = index::title_search("ре", &[&snapshot], 60);
    println!("поиск по заголовкам «ре»: {:?} — найдено {}", started.elapsed(), hits.len());

    let started = Instant::now();
    let backlinks = index.backlinks("Страница 1", &snapshot.space).unwrap();
    println!("обратные ссылки: {:?} — найдено {}", started.elapsed(), backlinks.len());

    let library = LibrarySnapshot::new(vec![snapshot.clone()]);
    let started = Instant::now();
    let issues = checker::check(&snapshot, &library);
    println!("folio check: {:?} — проблем {}", started.elapsed(), issues.len());

    let target = snapshot.pages_titled("Страница 1")[0].path.clone();
    let started = Instant::now();
    let result = PageOperations::new(&snapshot).rename_page(&target, "Страница первая", &library).unwrap();
    println!("переименование с починкой ссылок: {:?} — файлов {}", started.elapsed(), result.updated_files);
    let _ = Local::now();
}

/// Дерево до четырёх уровней, у каждой страницы шапка, текст на 2–8 КБ и пара ссылок.
fn generate(root: &Path, count: usize) {
    fs::create_dir_all(root).unwrap();
    write_page(root, "Работа", 0);
    let mut folders = vec![root.to_path_buf()];
    for number in 1..=count {
        let parent = folders[(number * 7919) % folders.len()].clone();
        let folder = parent.join(format!("Страница {number}"));
        fs::create_dir_all(&folder).unwrap();
        write_page(&folder, &format!("Страница {number}"), number);
        if folders.len() < count / 3 && folder.components().count() < root.components().count() + 4 {
            folders.push(folder);
        }
    }
}

fn write_page(folder: &Path, title: &str, seed: usize) {
    let mut body = String::new();
    let paragraphs = 4 + seed % 12;
    for paragraph in 0..paragraphs {
        for word in 0..60 {
            body.push_str(WORDS[(seed * 31 + paragraph * 7 + word) % WORDS.len()]);
            body.push(' ');
        }
        body.push_str("\n\n");
    }
    body.push_str(&format!("См. [[Страница {}]] и [[Страница 1]].\n", (seed * 13) % 997 + 1));
    let lines = vec![
        format!("id: {:012x}", seed),
        format!("title: {}", FrontmatterWriter::scalar(title)),
        "created: 2026-10-01".to_owned(),
        format!("status: {}", if seed % 3 == 0 { "В работе" } else { "Черновик" }),
        "labels: [тест, нагрузка]".to_owned(),
    ];
    fs::write(folder.join("index.md"), FrontmatterWriter::document(&lines, &body)).unwrap();
}
