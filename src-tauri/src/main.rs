// rdsconnect — IAM-native MySQL client.
// The Rust core owns all AWS + DB I/O and secrets; the UI only calls typed commands.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod aws;
mod commands;
mod connections;
mod db;
mod ssh;
mod sso;
mod state;
mod token;

use tauri::Manager;

use state::AppState;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let ca_bundle_path = state::resolve_ca_bundle_path(app.handle());
            app.manage(AppState::new(ca_bundle_path));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            // Auth
            commands::sso_login,
            commands::sso_status,
            commands::list_accounts,
            commands::list_account_roles,
            commands::list_db_instances,
            // Database
            commands::connect_db,
            commands::disconnect_db,
            commands::run_query,
            commands::list_schemas,
            commands::list_tables,
            commands::list_columns,
            commands::export_csv,
            // Persistence
            commands::list_saved_connections,
            commands::save_connection,
            commands::delete_connection,
            commands::list_history,
            commands::append_history,
            // Settings
            commands::get_settings,
            commands::update_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running rdsconnect");
}
