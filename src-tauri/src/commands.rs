// Tauri command handlers — thin, typed wrappers over the core modules.
// The UI only ever talks to these; it never sees AWS creds or raw tokens.

use serde::Serialize;
use tauri::State;

use crate::aws::{self, Account, DbInstance};
use crate::connections::{
    self, HistoryEntry, Settings,
};
use crate::db::{self, ColumnInfo, QueryResult};
use crate::sso;
use crate::state::{AppResult, AppState, ConnectionMeta};

/// Health check — proves the UI<->core round trip (build order step 1).
#[tauri::command]
pub fn ping() -> String {
    "pong".into()
}

// ---- auth ----

#[derive(Serialize)]
pub struct SsoStatus {
    pub logged_in: bool,
}

#[tauri::command]
pub async fn sso_status(state: State<'_, AppState>) -> AppResult<SsoStatus> {
    let settings = connections::load_settings().await?;
    // Try to restore silently from cache before reporting status.
    let logged_in = sso::try_restore(&state, &settings.sso_start_url).await?;
    Ok(SsoStatus { logged_in })
}

#[tauri::command]
pub async fn sso_login(state: State<'_, AppState>) -> AppResult<SsoStatus> {
    let settings = connections::load_settings().await?;
    // Reuse a cached token if we can, else run the browser device-auth flow.
    if !sso::try_restore(&state, &settings.sso_start_url).await? {
        sso::login(&state, &settings.sso_start_url, &settings.sso_region).await?;
    }
    Ok(SsoStatus { logged_in: true })
}

#[tauri::command]
pub async fn list_accounts(state: State<'_, AppState>) -> AppResult<Vec<Account>> {
    let settings = connections::load_settings().await?;
    aws::list_accounts(&state, &settings.sso_region).await
}

#[tauri::command]
pub async fn list_account_roles(
    state: State<'_, AppState>,
    account_id: String,
) -> AppResult<Vec<String>> {
    let settings = connections::load_settings().await?;
    aws::list_account_roles(&state, &settings.sso_region, &account_id).await
}

#[tauri::command]
pub async fn list_db_instances(
    state: State<'_, AppState>,
    account_id: String,
    role_name: String,
    region: String,
) -> AppResult<Vec<DbInstance>> {
    let settings = connections::load_settings().await?;
    aws::describe_db_instances(&state, &settings.sso_region, &account_id, &role_name, &region).await
}

// ---- db ----

#[tauri::command]
pub async fn connect_db(state: State<'_, AppState>, meta: ConnectionMeta) -> AppResult<()> {
    let settings = connections::load_settings().await?;
    db::connect(&state, meta, &settings.sso_region).await
}

#[tauri::command]
pub async fn disconnect_db(state: State<'_, AppState>, id: String) -> AppResult<()> {
    db::disconnect(&state, &id).await
}

#[tauri::command]
pub async fn run_query(
    state: State<'_, AppState>,
    id: String,
    sql: String,
) -> AppResult<QueryResult> {
    let settings = connections::load_settings().await?;
    let result = db::run_query(&state, &id, &sql, &settings.sso_region).await;

    // Record history (best-effort; never fail the query because history write failed).
    let entry = HistoryEntry {
        connection_id: id.clone(),
        sql: sql.clone(),
        at: chrono::Utc::now().to_rfc3339(),
        rows: result.as_ref().map(|r| r.rows_affected).unwrap_or(0),
        elapsed_ms: result.as_ref().map(|r| r.elapsed_ms).unwrap_or(0),
        ok: result.is_ok(),
    };
    let _ = connections::append_history(entry).await;

    result
}

#[tauri::command]
pub async fn list_schemas(state: State<'_, AppState>, id: String) -> AppResult<Vec<String>> {
    let settings = connections::load_settings().await?;
    db::list_schemas(&state, &id, &settings.sso_region).await
}

#[tauri::command]
pub async fn list_tables(
    state: State<'_, AppState>,
    id: String,
    schema: String,
) -> AppResult<Vec<String>> {
    let settings = connections::load_settings().await?;
    db::list_tables(&state, &id, &schema, &settings.sso_region).await
}

#[tauri::command]
pub async fn list_columns(
    state: State<'_, AppState>,
    id: String,
    schema: String,
    table: String,
) -> AppResult<Vec<ColumnInfo>> {
    let settings = connections::load_settings().await?;
    db::list_columns(&state, &id, &schema, &table, &settings.sso_region).await
}

// ---- persistence ----

#[tauri::command]
pub async fn list_saved_connections() -> AppResult<Vec<ConnectionMeta>> {
    connections::load_connections().await
}

#[tauri::command]
pub async fn save_connection(meta: ConnectionMeta) -> AppResult<Vec<ConnectionMeta>> {
    connections::save_connection(meta).await
}

#[tauri::command]
pub async fn delete_connection(id: String) -> AppResult<Vec<ConnectionMeta>> {
    connections::delete_connection(&id).await
}

#[tauri::command]
pub async fn list_history() -> AppResult<Vec<HistoryEntry>> {
    connections::load_history().await
}

#[tauri::command]
pub async fn append_history(entry: HistoryEntry) -> AppResult<()> {
    connections::append_history(entry).await
}

// ---- settings ----

#[tauri::command]
pub async fn get_settings() -> AppResult<Settings> {
    connections::load_settings().await
}

#[tauri::command]
pub async fn update_settings(settings: Settings) -> AppResult<Settings> {
    connections::save_settings(&settings).await?;
    Ok(settings)
}
