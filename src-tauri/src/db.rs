// MySQL connect + query + schema introspection.
//
// Auth is IAM: the password is a freshly generated RDS token (15-min validity) and every
// connection is TLS with the bundled Amazon RDS CA. Because the token expires, the pool is
// rebuilt transparently: `run_query` retries once on a connection-level failure, regenerating
// creds -> token -> pool. Existing sessions stay alive; only new connections need a fresh token.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlRow, MySqlSslMode};
use sqlx::{Column, Row, TypeInfo, ValueRef};

use crate::aws::get_role_credentials;
use crate::state::{ActiveConnection, AppError, AppResult, AppState, ConnectionMeta};
use crate::token;

#[derive(Serialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub rows_affected: u64,
    /// Wall-clock query time in milliseconds.
    pub elapsed_ms: u128,
}

/// Path to the bundled RDS CA. In dev it's next to the source; in a bundle Tauri resolves it,
/// but resolving needs the AppHandle — for the core we read the resource path via env/relative.
fn ca_path() -> AppResult<PathBuf> {
    // Prefer an explicit override (set by the packaged app), else the in-repo resource.
    if let Ok(p) = std::env::var("BIFROST_RDS_CA") {
        return Ok(PathBuf::from(p));
    }
    let local = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/global-bundle.pem");
    if local.exists() {
        return Ok(local);
    }
    Err(AppError::Db(
        "RDS CA bundle not found; set BIFROST_RDS_CA or add resources/global-bundle.pem".into(),
    ))
}

/// Build a fresh pool for `meta`, generating a new auth token. `sso_region` is where the SSO
/// portal lives (login region), distinct from the DB's region.
async fn build_pool(
    state: &AppState,
    meta: &ConnectionMeta,
    sso_region: &str,
) -> AppResult<sqlx::MySqlPool> {
    let creds =
        get_role_credentials(state, sso_region, &meta.account_id, &meta.role_name).await?;
    let auth_token = token::generate(&creds, &meta.region, &meta.host, meta.port, &meta.db_user)?;

    let mut opts = MySqlConnectOptions::new()
        .host(&meta.host)
        .port(meta.port)
        .username(&meta.db_user)
        .password(&auth_token)
        .ssl_mode(MySqlSslMode::VerifyIdentity)
        .ssl_ca(ca_path()?);
    if let Some(db) = &meta.default_database {
        opts = opts.database(db);
    }

    MySqlPoolOptions::new()
        .max_connections(4)
        // Recycle before the token would expire so new connections always re-auth.
        .max_lifetime(std::time::Duration::from_secs(10 * 60))
        .acquire_timeout(std::time::Duration::from_secs(20))
        .connect_with(opts)
        .await
        .map_err(|e| AppError::Db(format!("connect: {e}")))
}

/// Open (or replace) a connection for `meta`, storing it in state keyed by `meta.id`.
pub async fn connect(
    state: &AppState,
    meta: ConnectionMeta,
    sso_region: &str,
) -> AppResult<()> {
    let pool = build_pool(state, &meta, sso_region).await?;
    // Prove it works before we store it.
    sqlx::query("SELECT 1")
        .execute(&pool)
        .await
        .map_err(|e| AppError::Db(format!("handshake: {e}")))?;

    let active = Arc::new(ActiveConnection {
        pool: tokio::sync::RwLock::new(pool),
        meta: meta.clone(),
    });
    state.connections.write().await.insert(meta.id.clone(), active);
    Ok(())
}

pub async fn disconnect(state: &AppState, id: &str) -> AppResult<()> {
    if let Some(active) = state.connections.write().await.remove(id) {
        active.pool.read().await.close().await;
    }
    Ok(())
}

fn get_active(
    state_map: &std::collections::HashMap<String, Arc<ActiveConnection>>,
    id: &str,
) -> AppResult<Arc<ActiveConnection>> {
    state_map
        .get(id)
        .cloned()
        .ok_or_else(|| AppError::NoConnection(id.to_string()))
}

/// Rebuild the pool for an existing connection (transparent token refresh).
async fn rebuild(state: &AppState, active: &ActiveConnection, sso_region: &str) -> AppResult<()> {
    let pool = build_pool(state, &active.meta, sso_region).await?;
    let old = std::mem::replace(&mut *active.pool.write().await, pool);
    old.close().await;
    Ok(())
}

/// Run an arbitrary SQL statement. Retries once with a fresh token on connection failure.
pub async fn run_query(
    state: &AppState,
    id: &str,
    sql: &str,
    sso_region: &str,
) -> AppResult<QueryResult> {
    let rows = fetch_with_retry(state, id, sql, &[], sso_region).await?;
    Ok(rows_to_result(&rows.rows, rows.elapsed_ms))
}

struct RawRows {
    rows: Vec<MySqlRow>,
    elapsed_ms: u128,
}

/// Execute `sql` with positional string binds, retrying once with a fresh token on a
/// connection-level failure. All parameterized queries funnel through here so user-supplied
/// values (schema/table names, etc.) are always bound, never interpolated.
async fn fetch_with_retry(
    state: &AppState,
    id: &str,
    sql: &str,
    binds: &[&str],
    sso_region: &str,
) -> AppResult<RawRows> {
    let active = {
        let map = state.connections.read().await;
        get_active(&map, id)?
    };

    match execute(&active, sql, binds).await {
        Ok(r) => Ok(r),
        Err(e) if is_connection_error(&e) => {
            // Token likely expired and the pool couldn't re-auth — rebuild and retry once.
            rebuild(state, &active, sso_region).await?;
            execute(&active, sql, binds).await
        }
        Err(e) => Err(e),
    }
}

async fn execute(active: &ActiveConnection, sql: &str, binds: &[&str]) -> AppResult<RawRows> {
    let pool = active.pool.read().await.clone();
    let started = std::time::Instant::now();
    let mut q = sqlx::query(sql);
    for b in binds {
        q = q.bind(*b);
    }
    let rows = q
        .fetch_all(&pool)
        .await
        .map_err(|e| AppError::Db(e.to_string()))?;
    Ok(RawRows {
        rows,
        elapsed_ms: started.elapsed().as_millis(),
    })
}

fn rows_to_result(rows: &[MySqlRow], elapsed_ms: u128) -> QueryResult {
    let columns = rows
        .first()
        .map(|r| r.columns().iter().map(|c| c.name().to_string()).collect())
        .unwrap_or_default();
    QueryResult {
        columns,
        rows: rows.iter().map(row_to_strings).collect(),
        rows_affected: rows.len() as u64,
        elapsed_ms,
    }
}

/// Pull the first column of each row as a String (for single-column introspection queries).
fn first_col_strings(rows: &[MySqlRow]) -> Vec<String> {
    rows.iter()
        .filter_map(|r| r.try_get::<String, _>(0).ok())
        .collect()
}

/// Render each column as an optional String for the UI grid. Numbers/dates are stringified;
/// binary/unknown types fall back to a placeholder.
fn row_to_strings(row: &MySqlRow) -> Vec<Option<String>> {
    (0..row.len())
        .map(|i| {
            let raw = row.try_get_raw(i).ok();
            if raw.as_ref().map(|r| r.is_null()).unwrap_or(true) {
                return None;
            }
            let ty = raw.as_ref().map(|r| r.type_info().name().to_string()).unwrap_or_default();
            // Try the common scalar decodings in order.
            if let Ok(v) = row.try_get::<String, _>(i) {
                Some(v)
            } else if let Ok(v) = row.try_get::<i64, _>(i) {
                Some(v.to_string())
            } else if let Ok(v) = row.try_get::<f64, _>(i) {
                Some(v.to_string())
            } else if let Ok(v) = row.try_get::<bool, _>(i) {
                Some(v.to_string())
            } else if let Ok(v) = row.try_get::<chrono::NaiveDateTime, _>(i) {
                Some(v.to_string())
            } else if let Ok(v) = row.try_get::<Vec<u8>, _>(i) {
                Some(format!("<{} bytes {}>", v.len(), ty))
            } else {
                Some(format!("<{ty}>"))
            }
        })
        .collect()
}

fn is_connection_error(e: &AppError) -> bool {
    let AppError::Db(msg) = e else { return false };
    let m = msg.to_lowercase();
    m.contains("connection")
        || m.contains("broken pipe")
        || m.contains("access denied")
        || m.contains("timed out")
        || m.contains("io error")
}

// ---- schema introspection via information_schema ----

pub async fn list_schemas(state: &AppState, id: &str, sso_region: &str) -> AppResult<Vec<String>> {
    let rows = fetch_with_retry(
        state,
        id,
        "SELECT schema_name FROM information_schema.schemata \
         WHERE schema_name NOT IN ('information_schema','performance_schema','mysql','sys') \
         ORDER BY schema_name",
        &[],
        sso_region,
    )
    .await?;
    Ok(first_col_strings(&rows.rows))
}

pub async fn list_tables(
    state: &AppState,
    id: &str,
    schema: &str,
    sso_region: &str,
) -> AppResult<Vec<String>> {
    let rows = fetch_with_retry(
        state,
        id,
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = ? ORDER BY table_name",
        &[schema],
        sso_region,
    )
    .await?;
    Ok(first_col_strings(&rows.rows))
}

#[derive(Serialize)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub key: String,
}

pub async fn list_columns(
    state: &AppState,
    id: &str,
    schema: &str,
    table: &str,
    sso_region: &str,
) -> AppResult<Vec<ColumnInfo>> {
    let rows = fetch_with_retry(
        state,
        id,
        "SELECT column_name, column_type, is_nullable, column_key \
         FROM information_schema.columns \
         WHERE table_schema = ? AND table_name = ? ORDER BY ordinal_position",
        &[schema, table],
        sso_region,
    )
    .await?;
    Ok(rows
        .rows
        .iter()
        .map(|row| ColumnInfo {
            name: row.try_get::<String, _>(0).unwrap_or_default(),
            data_type: row.try_get::<String, _>(1).unwrap_or_default(),
            nullable: row.try_get::<String, _>(2).ok().as_deref() == Some("YES"),
            key: row.try_get::<String, _>(3).unwrap_or_default(),
        })
        .collect())
}
