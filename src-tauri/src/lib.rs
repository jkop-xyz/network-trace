//! The desktop shell: owns running Sessions, exposes the core to the UI as
//! commands, and handles the tray, printing and file saving.

mod commands;
mod desktop;
mod settings;
mod state;

use state::AppState;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

/// The URI scheme that serves the Report being printed.
pub const REPORT_SCHEME: &str = "ntreport";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    // Must be the first plugin: a second launch focuses this instance (and
    // starts any Targets it was given here) instead of running a second
    // recorder. Left out of debug builds, which would otherwise hand their
    // arguments to an installed copy that is running at the same time.
    #[cfg(not(debug_assertions))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
        show_main(app);
        let targets = targets_from(args.into_iter().skip(1));
        if targets.is_empty() {
            // Most likely someone started a freshly downloaded version while
            // this one runs in the tray; tell them which version this is.
            let _ = app.emit("second-launch", app.package_info().version.to_string());
        }
        start_targets(app.clone(), targets);
    }));
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .register_uri_scheme_protocol(REPORT_SCHEME, |ctx, _request| {
            let html = ctx.app_handle().state::<AppState>().print_html.lock().unwrap().clone();
            tauri::http::Response::builder()
                .header("Content-Type", "text/html; charset=utf-8")
                .body(html.into_bytes())
                .unwrap()
        })
        .setup(|app| {
            let state = AppState::load(app.handle())?;
            app.manage(state);
            build_tray(app)?;
            app.state::<AppState>().update_tray();
            desktop::integrate();
            start_targets(app.handle().clone(), targets_from(std::env::args().skip(1)));
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window keeps Sessions running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                    explain_tray_once(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::settings_get,
            commands::settings_set,
            commands::sessions_list,
            commands::session_start,
            commands::session_stop,
            commands::session_resume,
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
            commands::reveal_file,
            commands::ip_owner,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Network Trace", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit (stops all Sessions)", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().expect("bundled icon"))
        .tooltip("Network Trace")
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

/// The first time the window is closed, say that monitoring continues.
fn explain_tray_once(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let mut settings = state.settings.lock().unwrap();
    if settings.tray_notice_shown {
        return;
    }
    settings.tray_notice_shown = true;
    let _ = settings.save(&state.settings_path);
    app.dialog()
        .message(
            "Network Trace keeps monitoring in the background.\n\n\
             Open it again from the tray icon. To stop all Sessions and exit, choose \"Quit\" in the tray menu.",
        )
        .title("Still running")
        .kind(MessageDialogKind::Info)
        .show(|_| {});
}

/// Targets given on the command line, e.g. `network-trace 1.1.1.1 example.com`.
fn targets_from(args: impl Iterator<Item = String>) -> Vec<String> {
    args.filter(|a| !a.starts_with('-')).collect()
}

fn start_targets(app: tauri::AppHandle, targets: Vec<String>) {
    if targets.is_empty() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        for target in targets {
            if let Err(e) = commands::session_start(app.state::<AppState>(), target.clone()).await {
                eprintln!("could not start {target}: {e}");
            }
        }
        let _ = app.emit("sessions-changed", ());
    });
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}
