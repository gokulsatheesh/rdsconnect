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
    // rustls 0.23 is compiled with more than one crypto provider (aws-lc-rs + ring) via our
    // transitive deps (the AWS SDK and sqlx), so it can't auto-select one and panics on the first
    // TLS handshake with "Could not automatically determine the process-level CryptoProvider".
    // Install aws-lc-rs as the process-wide default explicitly, before anything opens a TLS
    // connection. Ignoring the result is fine — an Err just means it was already installed.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

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
