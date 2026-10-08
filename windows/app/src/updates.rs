//! Обновления с GitHub Releases: проверка, загрузка в фоне и установка при выходе или сразу.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct UpdateState {
    update: Mutex<Option<Update>>,
    bytes: Mutex<Option<(String, Vec<u8>)>>,
    install_on_exit: AtomicBool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    version: String,
    current_version: String,
    notes: String,
    downloaded: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEnvironment {
    current_version: String,
    /// Обновляется только собранное приложение, а не отладочный запуск из исходников.
    can_update: bool,
}

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[tauri::command]
pub fn update_info(app: AppHandle) -> UpdateEnvironment {
    UpdateEnvironment { current_version: app.package_info().version.to_string(), can_update: !cfg!(debug_assertions) }
}

/// Последний выпуск новее установленного; `None` — обновлений нет.
#[tauri::command]
pub async fn update_check(app: AppHandle, state: State<'_, UpdateState>) -> Result<Option<UpdateInfo>, String> {
    let update = app.updater().map_err(text)?.check().await.map_err(text)?;
    let downloaded = |version: &str| state.bytes.lock().unwrap().as_ref().is_some_and(|(ready, _)| ready == version);
    let info = update.as_ref().map(|update| UpdateInfo {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes: update.body.clone().unwrap_or_default(),
        downloaded: downloaded(&update.version),
    });
    *state.update.lock().unwrap() = update;
    Ok(info)
}

/// Скачивает выпуск; ход загрузки приходит событиями `update-progress` (доля от 0 до 1).
#[tauri::command]
pub async fn update_download(app: AppHandle, state: State<'_, UpdateState>, install_on_exit: bool) -> Result<(), String> {
    let update = state.update.lock().unwrap().clone().ok_or("обновление не найдено — проверьте ещё раз")?;
    let bytes = download(&app, &update).await?;
    *state.bytes.lock().unwrap() = Some((update.version.clone(), bytes));
    state.install_on_exit.store(install_on_exit, Ordering::SeqCst);
    let _ = app.emit("update-ready", &update.version);
    Ok(())
}

async fn download(app: &AppHandle, update: &Update) -> Result<Vec<u8>, String> {
    let progress = app.clone();
    let mut received = 0u64;
    let mut reported = 0u32;
    update
        .download(
            move |chunk, total| {
                received += chunk as u64;
                let Some(total) = total.filter(|total| *total > 0) else { return };
                let percent = (received * 100 / total) as u32;
                if percent != reported {
                    reported = percent;
                    let _ = progress.emit("update-progress", percent as f64 / 100.0);
                }
            },
            || {},
        )
        .await
        .map_err(text)
}

/// Ставит обновление сейчас: установщик закрывает приложение и запускает новую версию.
#[tauri::command]
pub async fn update_install(app: AppHandle, state: State<'_, UpdateState>) -> Result<(), String> {
    let update = state.update.lock().unwrap().clone().ok_or("обновление не найдено — проверьте ещё раз")?;
    let ready = state.bytes.lock().unwrap().take().filter(|(version, _)| *version == update.version);
    let bytes = match ready {
        Some((_, bytes)) => bytes,
        None => download(&app, &update).await?,
    };
    state.install_on_exit.store(false, Ordering::SeqCst);
    update.install(bytes).map_err(text)?;
    app.restart();
}

pub fn install_pending_on_exit(app: &AppHandle) {
    let state = app.state::<UpdateState>();
    if !state.install_on_exit.swap(false, Ordering::SeqCst) {
        return;
    }
    let update = state.update.lock().unwrap().clone();
    let bytes = state.bytes.lock().unwrap().take();
    if let (Some(update), Some((version, bytes))) = (update, bytes) {
        if update.version == version {
            let _ = update.install(bytes);
        }
    }
}
