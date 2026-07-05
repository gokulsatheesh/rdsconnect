// rdsconnect — IAM-native MySQL client.
// The Rust core owns all AWS + DB I/O and secrets; the UI only calls typed commands.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod aws;
mod commands;
mod connections;
mod db;
mod sso;
mod state;
mod token;

use state::AppState;

fn main() {
    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            // --- health / debug ---
            commands::ping,
            // --- auth core (step 2) ---
            commands::sso_login,
            commands::sso_status,
            commands::list_accounts,
            commands::list_account_roles,
            commands::list_db_instances,
            // --- db (step 3) ---
            commands::connect_db,
            commands::disconnect_db,
            commands::run_query,
            commands::list_schemas,
            commands::list_tables,
            commands::list_columns,
            // --- persistence (steps 4/6) ---
            commands::list_saved_connections,
            commands::save_connection,
            commands::delete_connection,
            commands::list_history,
            commands::append_history,
            // --- settings ---
            commands::get_settings,
            commands::update_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running rdsconnect");
}
