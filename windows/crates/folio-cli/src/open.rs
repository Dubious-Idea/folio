//! Открытие ссылки `folio://` в приложении через оболочку Windows.

use crate::workspace::Failure;

#[cfg(windows)]
pub fn open_url(url: &str) -> Result<(), Failure> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide = |text: &str| text.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let verb = wide("open");
    let target = wide(url);
    // ShellExecuteW возвращает число больше 32 при успехе, иначе код ошибки.
    let result = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), target.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
    let code = result as isize;
    if code > 32 {
        return Ok(());
    }
    const NO_ASSOCIATION: isize = 31;
    let message = if code == NO_ASSOCIATION {
        "ссылки folio:// некому открыть — установите приложение Folio".to_owned()
    } else {
        format!("оболочка Windows не открыла ссылку (код {code})")
    };
    Err(Failure::Usage(format!("open: {message}")))
}

#[cfg(not(windows))]
pub fn open_url(url: &str) -> Result<(), Failure> {
    let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let status = std::process::Command::new(program).arg(url).status()?;
    if status.success() { Ok(()) } else { Err(Failure::Usage(format!("open: {program} завершился с кодом {status}"))) }
}
