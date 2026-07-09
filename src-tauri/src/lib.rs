mod commands;
mod secrets;
mod ssh;
mod state;
mod storage;

use tauri::Manager;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir().expect("failed to resolve app data dir");
            std::fs::create_dir_all(&app_data_dir).expect("failed to create app data dir");
            let connections_path = app_data_dir.join("connections.json");
            let store = storage::ConnectionsStore::load(connections_path);
            app.manage(AppState::new(store));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::connections::list_connections,
            commands::connections::save_connection,
            commands::connections::delete_connection,
            commands::connections::duplicate_connection,
            commands::connections::save_credential,
            commands::connections::has_credential,
            commands::session::open_session,
            commands::session::send_input,
            commands::session::resize_session,
            commands::session::close_session,
            commands::session::test_connection,
            commands::sftp::sftp_canonicalize,
            commands::sftp::sftp_list_dir,
            commands::sftp::sftp_download,
            commands::sftp::sftp_upload,
            commands::sftp::sftp_mkdir,
            commands::sftp::sftp_delete,
            commands::sftp::sftp_rename,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
