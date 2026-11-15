//! The desktop shell: owns running Sessions, exposes the core to the UI as
//! commands, and handles the tray, printing and file saving.

mod commands;
mod settings;
mod state;

use state::AppState;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = AppState::load(app.handle())?;
            app.manage(state);
            build_tray(app)?;
            start_from_args(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window keeps Sessions running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::settings_get,
            commands::settings_set,
            commands::sessions_list,
            commands::session_start,
            commands::session_stop,
            commands::session_restart,
            commands::session_set_interval,
            commands::session_set_note,
            commands::sessions_delete,
            commands::session_view,
            commands::session_timeline,
            commands::export_csv,
            commands::report_context,
            commands::save_text,
            commands::print_report,
            commands::open_data_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show network-trace", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit (stops all Sessions)", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().expect("bundled icon"))
        .tooltip("network-trace")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "quit" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    app.state::<AppState>().stop_all().await;
                    app.exit(0);
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// `network-trace 1.1.1.1 example.com` starts a Session for each Target.
fn start_from_args(app: tauri::AppHandle) {
    let targets: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with('-')).collect();
    if targets.is_empty() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        for target in targets {
            if let Err(e) = commands::session_start(app.state::<AppState>(), target.clone()).await {
                eprintln!("could not start {target}: {e}");
            }
        }
        let _ = app.emit("session-stopped", ());
    });
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}
