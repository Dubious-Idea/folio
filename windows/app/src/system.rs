//! Связь с Windows: ссылки, Проводник, буфер обмена, утилита folio в PATH.

use std::path::{Path, PathBuf};

use folio_core::fsutil;
use folio_core::SpaceSnapshot;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

/// Программы, скрипты и ярлыки из текста страницы не запускаем — только показываем в Проводнике.
const RUNNABLE_EXTENSIONS: &[&str] = &[
    "exe", "com", "bat", "cmd", "ps1", "psm1", "psd1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "ws", "msi", "msp", "mst", "msc", "scr",
    "pif", "lnk", "url", "reg", "hta", "cpl", "jar", "py", "pyw", "rb", "pl", "sh", "appref-ms", "application", "gadget", "inf", "ins",
    "isp", "sct", "shb", "shs", "xbap", "settingcontent-ms", "library-ms", "search-ms", "appx", "appxbundle", "msix", "msixbundle", "dll",
    "sys", "chm", "hlp", "iso", "img", "vhd", "vhdx", "diagcab", "xll",
];

pub enum LinkTarget {
    Web(String),
    File(PathBuf),
    Reveal(PathBuf),
}

/// Ссылка из текста страницы: внешний адрес открывается в браузере, относительный путь — файлом рядом со страницей.
pub fn resolve_link(href: &str, snapshot: &SpaceSnapshot, page_path: &str) -> Result<Option<LinkTarget>, String> {
    if let Some((scheme, _)) = href.split_once(':').filter(|(scheme, _)| is_scheme(scheme)) {
        let scheme = scheme.to_ascii_lowercase();
        return Ok(["http", "https", "mailto"].contains(&scheme.as_str()).then(|| LinkTarget::Web(href.to_owned())));
    }
    let relative = href.split('#').next().unwrap_or(href);
    let decoded = percent_decode(relative).unwrap_or_else(|| relative.to_owned());
    let target = fsutil::normalize(&snapshot.folder_path(page_path).join(decoded.replace('/', "\\")));
    if !snapshot.space.contains(&target) || !target.exists() {
        return Err(format!("Файл «{relative}» не найден рядом со страницей"));
    }
    Ok(Some(target_for_file(target)))
}

/// Папки и всё, что может запуститься, показываем в Проводнике, остальное открываем.
pub fn target_for_file(path: PathBuf) -> LinkTarget {
    let extension = path.extension().map(|extension| extension.to_string_lossy().to_lowercase()).unwrap_or_default();
    if path.is_dir() || RUNNABLE_EXTENSIONS.contains(&extension.as_str()) { LinkTarget::Reveal(path) } else { LinkTarget::File(path) }
}

/// Буква диска `C:` — это путь, а не схема адреса.
fn is_scheme(scheme: &str) -> bool {
    scheme.len() > 1 && scheme.starts_with(|first: char| first.is_ascii_alphabetic())
        && scheme.chars().all(|character| character.is_ascii_alphanumeric() || "+.-".contains(character))
}

fn percent_decode(text: &str) -> Option<String> {
    if !text.contains('%') {
        return Some(text.to_owned());
    }
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            decoded.push(u8::from_str_radix(text.get(index + 1..index + 3)?, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

pub fn open_target(app: &AppHandle, target: LinkTarget) -> Result<(), String> {
    match target {
        LinkTarget::Web(url) => app.opener().open_url(url, None::<&str>).map_err(|error| error.to_string()),
        LinkTarget::File(path) => open_path(app, &path),
        LinkTarget::Reveal(path) => reveal(app, &path),
    }
}

pub fn open_path(app: &AppHandle, path: &Path) -> Result<(), String> {
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(|error| error.to_string())
}

pub fn reveal(app: &AppHandle, path: &Path) -> Result<(), String> {
    app.opener().reveal_item_in_dir(path).map_err(|error| error.to_string())
}

pub fn copy_text(text: &str) -> Result<(), String> {
    clipboard_win::set_clipboard_string(text).map_err(|error| error.to_string())
}

/// Папка с folio.exe: в установленном приложении — `bin` рядом с Folio.exe, при разработке — рядом с бинарником.
pub fn cli_folder(app: &AppHandle) -> Option<PathBuf> {
    let candidates = [
        app.path().resource_dir().ok().map(|folder| folder.join("bin")),
        std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)),
    ];
    candidates.into_iter().flatten().find(|folder| folder.join("folio.exe").is_file()).map(|folder| fsutil::normalize(&folder))
}

#[cfg(windows)]
pub mod path_variable {
    //! Пользовательская переменная PATH в реестре (HKCU\Environment).

    use std::path::Path;

    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, LPARAM, WPARAM};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_EXPAND_SZ, REG_SZ, RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE};

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn open(access: u32) -> Option<Key> {
        let mut key: HKEY = std::ptr::null_mut();
        let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide("Environment").as_ptr(), 0, access, &mut key) };
        (status == ERROR_SUCCESS).then_some(Key(key))
    }

    fn read() -> (String, u32) {
        let Some(key) = open(KEY_READ) else { return (String::new(), REG_EXPAND_SZ) };
        let name = wide("Path");
        let mut kind = 0u32;
        let mut size = 0u32;
        let status = unsafe { RegQueryValueExW(key.0, name.as_ptr(), std::ptr::null(), &mut kind, std::ptr::null_mut(), &mut size) };
        if status != ERROR_SUCCESS || size == 0 {
            return (String::new(), REG_EXPAND_SZ);
        }
        let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
        let status = unsafe { RegQueryValueExW(key.0, name.as_ptr(), std::ptr::null(), &mut kind, buffer.as_mut_ptr().cast(), &mut size) };
        if status != ERROR_SUCCESS {
            return (String::new(), REG_EXPAND_SZ);
        }
        let length = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());
        (String::from_utf16_lossy(&buffer[..length]), if kind == REG_SZ { REG_SZ } else { REG_EXPAND_SZ })
    }

    fn same_folder(entry: &str, folder: &Path) -> bool {
        let folder = folder.to_string_lossy();
        entry.trim().trim_end_matches('\\').to_lowercase() == folder.trim_end_matches('\\').to_lowercase()
    }

    pub fn contains(folder: &Path) -> bool {
        read().0.split(';').any(|entry| same_folder(entry, folder))
    }

    /// Добавляет папку в конец PATH и сообщает открытым программам, что окружение поменялось.
    pub fn add(folder: &Path) -> Result<(), String> {
        let (current, kind) = read();
        let mut entries: Vec<&str> = current.split(';').filter(|entry| !entry.trim().is_empty()).collect();
        if entries.iter().any(|entry| same_folder(entry, folder)) {
            return Ok(());
        }
        let folder_text = folder.to_string_lossy();
        entries.push(&folder_text);
        let value = wide(&entries.join(";"));
        let key = open(KEY_WRITE).ok_or("нет доступа к переменным среды пользователя")?;
        let status = unsafe {
            RegSetValueExW(key.0, wide("Path").as_ptr(), 0, kind, value.as_ptr().cast(), (value.len() * 2) as u32)
        };
        if status != ERROR_SUCCESS {
            return Err(format!("не удалось записать PATH (код {status})"));
        }
        let environment = wide("Environment");
        unsafe {
            SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0 as WPARAM, environment.as_ptr() as LPARAM, SMTO_ABORTIFHUNG, 2000, std::ptr::null_mut());
        }
        Ok(())
    }
}
