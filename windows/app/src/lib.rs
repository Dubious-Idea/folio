//! Приложение Folio для Windows: окно на WebView2, данные — в `folio-core`.

mod commands;
mod dto;
mod library;
mod protocol;
mod system;
mod updates;
mod watcher;

use std::sync::Mutex;

use tauri::{Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_deep_link::DeepLinkExt;

use library::Library;

/// Ссылки `folio://`, пришедшие до того, как окно было готово их принять, и окно, которое видели последним.
#[derive(Default)]
pub struct Session {
    pending_links: Mutex<Vec<String>>,
    focused_window: Mutex<Option<String>>,
}

pub fn run() {
    let app = tauri::Builder::default()
        // Второй запуск (например, ссылка folio:// из утилиты) отдаёт аргументы уже открытому окну.
        .plugin(tauri_plugin_single_instance::init(|app, _arguments, _directory| focus_window(app)))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .register_asynchronous_uri_scheme_protocol(protocol::SCHEME, protocol::handle)
        .manage(Session::default())
        .manage(updates::UpdateState::default())
        .setup(|app| {
            let library = Library::start(app.handle().clone());
            app.manage(library);

            // Установщик регистрирует схему folio:// сам; при запуске без установки регистрируем её здесь.
            let _ = app.deep_link().register_all();
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                let session = app.state::<Session>();
                session.pending_links.lock().unwrap().extend(urls.into_iter().map(|url| url.to_string()));
            }
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    deliver_link(&handle, url.to_string());
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::Focused(true) = event {
                *window.state::<Session>().focused_window.lock().unwrap() = Some(window.label().to_owned());
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_library,
            commands::get_pages_index,
            commands::get_page,
            commands::search,
            commands::backlinks,
            commands::page_attachments,
            commands::page_history,
            commands::templates,
            commands::create_page,
            commands::rename_page,
            commands::trash_page,
            commands::commit_space,
            commands::editor_save,
            commands::set_properties,
            commands::save_attachment,
            commands::copy_attachments,
            commands::open_link,
            commands::open_file,
            commands::reveal_page,
            commands::reveal_path,
            commands::copy_page_path,
            commands::create_default_spaces,
            commands::create_space,
            commands::add_space,
            commands::remove_space,
            commands::forget_missing,
            commands::default_spaces_folder,
            commands::new_window,
            commands::take_pending_links,
            commands::cli_status,
            commands::cli_install,
            updates::update_check,
            updates::update_download,
            updates::update_install,
            updates::update_info,
        ])
        .build(tauri::generate_context!())
        .expect("приложение Folio не запустилось");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            // Скачанное заранее обновление ставится при выходе.
            updates::install_pending_on_exit(handle);
        }
    });
}

fn focus_window(app: &tauri::AppHandle) {
    let label = app.state::<Session>().focused_window.lock().unwrap().clone();
    let window = label.and_then(|label| app.get_webview_window(&label)).or_else(|| app.webview_windows().into_values().next());
    if let Some(window) = window {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Ссылка `folio://open` уходит в последнее активное окно.
fn deliver_link(app: &tauri::AppHandle, url: String) {
    focus_window(app);
    let label = app.state::<Session>().focused_window.lock().unwrap().clone();
    let delivered = match label {
        Some(label) => app.emit_to(label.as_str(), "open-link", &url).is_ok(),
        None => false,
    };
    if !delivered {
        app.state::<Session>().pending_links.lock().unwrap().push(url);
    }
}
