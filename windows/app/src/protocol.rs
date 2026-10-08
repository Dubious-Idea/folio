//! Схема `folio` внутри окна: отдаёт вложения страниц (`http://folio.localhost/file/<пространство>/…`).
//! На Windows WebView2 видит свои схемы как `http://<схема>.localhost`.

use std::sync::Arc;

use folio_core::PageRef;
use folio_core::fsutil;
use folio_core::space::uuid_string;
use tauri::http::{Request, Response, StatusCode, header};
use tauri::{Manager, Runtime, UriSchemeContext, UriSchemeResponder};

use crate::library::Library;

pub const SCHEME: &str = "folio";
const FILE_PREFIX: &str = "/file/";

fn origin() -> &'static str {
    if cfg!(windows) { "http://folio.localhost" } else { "folio://localhost" }
}

/// Адрес папки страницы: от него веб-часть разрешает относительные пути вложений.
pub fn asset_base(page: &PageRef) -> String {
    let mut base = format!("{}{FILE_PREFIX}{}/", origin(), uuid_string(page.space_id));
    for segment in page.path.split('/').filter(|segment| !segment.is_empty()) {
        base.push_str(&encode_segment(segment));
        base.push('/');
    }
    base
}

fn encode_segment(segment: &str) -> String {
    let mut result = String::with_capacity(segment.len() * 3);
    for &byte in segment.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            result.push(byte as char);
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    result
}

fn decode_segment(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            decoded.push(u8::from_str_radix(segment.get(index + 1..index + 3)?, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

pub fn handle<R: Runtime>(context: UriSchemeContext<'_, R>, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let library = context.app_handle().state::<Arc<Library>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || responder.respond(respond(&library, &request)));
}

fn respond(library: &Library, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    match resolve(library, request.uri().path()).and_then(|file| std::fs::read(&file).ok().map(|data| (file, data))) {
        Some((file, data)) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime_type(&file))
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(header::CACHE_CONTROL, "no-store")
            .body(data)
            .unwrap_or_else(|_| not_found()),
        None => not_found(),
    }
}

fn not_found() -> Response<Vec<u8>> {
    Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()).unwrap()
}

/// Путь внутри пространства без выходов наверх.
fn resolve(library: &Library, path: &str) -> Option<std::path::PathBuf> {
    let rest = path.strip_prefix(FILE_PREFIX)?;
    let mut segments = rest.split('/').filter(|segment| !segment.is_empty());
    let space = uuid::Uuid::parse_str(segments.next()?).ok()?;
    let snapshot = library.snapshot(space)?;
    let mut file = snapshot.space.root.clone();
    for segment in segments {
        let decoded = decode_segment(segment)?;
        let is_unsafe = decoded == "." || decoded == ".." || decoded.contains(['\\', '/', ':']) || decoded.is_empty();
        if is_unsafe {
            return None;
        }
        file.push(decoded);
    }
    (file != snapshot.space.root && fsutil::is_within(&file, &snapshot.space.root) && file.is_file()).then_some(file)
}

fn mime_type(file: &std::path::Path) -> &'static str {
    let extension = file.extension().map(|extension| extension.to_string_lossy().to_lowercase()).unwrap_or_default();
    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        "tif" | "tiff" => "image/tiff",
        "pdf" => "application/pdf",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "txt" | "md" | "log" | "csv" => "text/plain; charset=utf-8",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_base_encodes_segments() {
        let page = PageRef::new(uuid::Uuid::nil(), "Проекты/Встреча 15.09");
        let base = asset_base(&page);
        assert!(base.starts_with("http://folio.localhost/file/00000000-0000-0000-0000-000000000000/"));
        assert!(base.ends_with("/%D0%92%D1%81%D1%82%D1%80%D0%B5%D1%87%D0%B0%2015.09/"));
        assert_eq!(decode_segment("%D0%92%D1%81%D1%82%D1%80%D0%B5%D1%87%D0%B0%2015.09").as_deref(), Some("Встреча 15.09"));
    }
}
