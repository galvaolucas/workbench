mod auth;
mod commands;
mod config;
mod db;
mod error;
mod github;
mod keychain;
mod notes;
mod notify;
mod state;
mod sync;
mod tray;

use std::sync::Arc;

use tauri::{Manager, WindowEvent};

use crate::auth::device_flow::DeviceFlow;
use crate::config::Host;
use crate::state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Dev convenience only. `tauri dev` runs the binary from src-tauri/, and
    // dotenvy walks up, so a `.env` at the repo root is found. Deliberately not
    // in release builds: a shipped app must not change how it authenticates
    // based on a stray file in whatever directory it was launched from.
    #[cfg(debug_assertions)]
    if let Err(e) = dotenvy::dotenv() {
        if !e.not_found() {
            eprintln!("ignoring unreadable .env: {e}");
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::auth_status,
            commands::auth_begin,
            commands::auth_cancel,
            commands::auth_logout,
            commands::send_test_notification,
            commands::hide_window,
            commands::open_external,
            commands::desk,
            commands::sync_now,
            commands::open_pull_request,
            commands::note_open,
            commands::note_save,
            commands::note_search,
        ])
        .setup(|app| {
            let db_path = app.path().app_data_dir()?.join("workbench.db");
            let conn = db::open(&db_path)?;

            let http = reqwest::Client::builder()
                .user_agent(config::USER_AGENT)
                .timeout(std::time::Duration::from_secs(30))
                .build()?;

            let host = Host::from_env();
            let auth = config::client_id().map(|id| {
                Arc::new(DeviceFlow::new(id, host.clone(), http.clone())) as Arc<dyn auth::AuthProvider>
            });
            if auth.is_none() {
                log::warn!("no WORKBENCH_GITHUB_CLIENT_ID set — sign-in is disabled");
            }

            app.manage(AppState::new(conn, http, host, auth));

            tray::build(app.handle())?;

            // Already connected from a previous run? Start polling immediately —
            // the window may never be opened today.
            let signed_in = {
                let state = app.state::<AppState>();
                let db = state.db.lock().unwrap();
                db::current_account(&db)?.is_some()
            };
            if signed_in {
                sync::spawn_loop(app.handle());
            }

            Ok(())
        })
        // Closing the window is how you put the app away, not how you quit it.
        // Everything that matters keeps running in the menu bar.
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            // Drives the polling cadence: fast while you're here, slow when
            // you're not, slower still once you've been gone a while.
            WindowEvent::Focused(focused) => {
                window.app_handle().state::<AppState>().set_focused(*focused);
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running Workbench");
}
