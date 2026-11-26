mod commands;
mod executions;
mod files;
mod headless;
mod mcp;
mod portal;
mod privacy;
mod server;
mod shell;
mod store;
mod tunnel;
mod worker;

use std::sync::Arc;
use store::Store;
use tauri::Manager;

pub use headless::run as run_headless;
pub use worker::run as run_execution_worker;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let state = Store::open(app.path().app_data_dir()?).map_err(std::io::Error::other)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::snapshot,
            commands::add_scope,
            commands::remove_scope,
            commands::rename_scope,
            commands::execution_detail,
            commands::cancel_execution,
            commands::save_settings,
            commands::start_server,
            commands::stop_server,
            commands::start_tunnel,
            commands::stop_tunnel,
            commands::get_token,
            commands::rotate_token,
            commands::dependencies,
            commands::portal_status
        ])
        .build(tauri::generate_context!())
        .expect("CloudMCP 启动失败")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                let store = app.state::<Arc<Store>>();
                store.cancel_commands();
                let live = store.live.lock().unwrap();
                if let Some(c) = &live.server {
                    c.cancel();
                }
                if let Some(c) = &live.tunnel {
                    c.cancel();
                }
            }
        });
}
