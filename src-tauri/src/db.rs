// MySQL connect + query + schema introspection.
//
// Auth is IAM: the password is a freshly generated RDS token (15-min validity) and every
// connection is TLS with the bundled Amazon RDS CA. Because the token expires, the pool is
// rebuilt transparently: `run_query` retries once on a connection-level failure, regenerating
// creds -> token -> pool. Existing sessions stay alive; only new connections need a fresh token.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlRow, MySqlSslMode};
use sqlx::{Column, ConnectOptions, Connection, Row, TypeInfo, ValueRef};

use crate::aws::get_role_credentials;
use crate::state::{
    ActiveConnection, AppError, AppResult, AppState, ConnectTarget, ConnectionMeta,
};
use crate::token;

#[derive(Serialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub rows_affected: u64,
    /// Wall-clock query time in milliseconds.
    pub elapsed_ms: u128,
}

/// Path to the bundled RDS CA, resolved once at startup via the Tauri resource dir (see
/// `state::resolve_ca_bundle_path`) and stashed in `AppState`. Never baked in at compile time —
/// `CARGO_MANIFEST_DIR` is only valid on the machine that *built* the binary, not the one running
/// a downloaded release.
fn ca_path(state: &AppState) -> AppResult<PathBuf> {
    state.ca_bundle_path.clone().ok_or_else(|| {
        AppError::Db(
            "RDS CA bundle not found; set RDSCONNECT_RDS_CA or add resources/global-bundle.pem"
                .into(),
        )
    })
}

/// How long a single MySQL/TLS handshake may take before we give up. Applies to the direct
/// validation connect below, so a stalled handshake fails in bounded time with a clear message.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Build a fresh pool for `meta`, generating a new auth token. `target` is where the socket
/// actually connects (the real endpoint, or a local SSH-forwarded port). `sso_region` is where
/// the SSO portal lives (login region), distinct from the DB's region.
async fn build_pool(
    state: &AppState,
    meta: &ConnectionMeta,
    target: &ConnectTarget,
    sso_region: &str,
) -> AppResult<sqlx::MySqlPool> {
    let creds = get_role_credentials(state, sso_region, &meta.account_id, &meta.role_name).await?;
    // The IAM token is always signed for the REAL RDS endpoint (its true identity), even when the
    // socket is tunneled through localhost — the DB validates the token against its own hostname.
    let auth_token = token::generate(&creds, &meta.region, &meta.host, meta.port, &meta.db_user)?;

    // Tunneled connections hit 127.0.0.1, whose name can't match the RDS cert, so drop from
    // VerifyIdentity to VerifyCa: the cert must still chain to the Amazon RDS CA (proving it's a
    // real RDS instance), we just can't hostname-pin it. The SSH hop already authenticated the path.
    let ssl_mode = if target.verify_identity {
        MySqlSslMode::VerifyIdentity
    } else {
        MySqlSslMode::VerifyCa
    };

    let mut opts = MySqlConnectOptions::new()
        .host(&target.host)
        .port(target.port)
        .username(&meta.db_user)
        .password(&auth_token)
        .ssl_mode(ssl_mode)
        .ssl_ca(ca_path(state)?)
        // RDS IAM auth uses MySQL's cleartext-password plugin: the auth "password" is the IAM
        // token, sent in the clear *inside* the TLS tunnel. sqlx disables that plugin by default
        // (cleartext without TLS is unsafe); enable it here — safe precisely because we mandate
        // TLS (VerifyIdentity/VerifyCa) above, so the token never travels unencrypted.
        .enable_cleartext_plugin(true);
    if let Some(db) = &meta.default_database {
        opts = opts.database(db);
    }

    // Validate with a single direct connection FIRST. A pool's eager `connect_with` retries
    // opening connections until `acquire_timeout` and then reports a generic
    // "pool timed out while waiting for an open connection" — discarding the real per-attempt
    // error (TLS verification failure, "Access denied" from IAM auth, wrong DB, etc.). Opening
    // one connection ourselves surfaces that actual error verbatim.
    validate_connection(&opts).await?;

    // The connection is proven to work, so build the pool lazily (no second handshake now).
    Ok(MySqlPoolOptions::new()
        .max_connections(4)
        // Recycle before the token would expire so new connections always re-auth.
        .max_lifetime(Duration::from_secs(10 * 60))
        .acquire_timeout(Duration::from_secs(20))
        .connect_lazy_with(opts))
}

/// Open one connection directly (bypassing the pool) so the real handshake/auth error is
/// surfaced instead of being masked by the pool's generic timeout. Runs `SELECT 1` to prove the
/// session is actually usable, then closes it.
async fn validate_connection(opts: &MySqlConnectOptions) -> AppResult<()> {
    let mut conn = tokio::time::timeout(CONNECT_TIMEOUT, opts.connect())
        .await
        .map_err(|_| {
            AppError::Db(format!(
                "connection timed out after {}s during the MySQL/TLS handshake — the host is \
                 reachable but the server isn't completing the connection",
                CONNECT_TIMEOUT.as_secs()
            ))
        })?
        .map_err(|e| AppError::Db(format!("connect: {e}")))?;

    let probe = sqlx::query("SELECT 1")
        .execute(&mut conn)
        .await
        .map_err(|e| AppError::Db(format!("handshake: {e}")));
    let _ = conn.close().await;
    probe.map(|_| ())
}

/// How long a preflight TCP dial waits before we declare the host unreachable. Comfortably
/// longer than a real handshake (sub-second when routable) but far shorter than the pool's
/// 20s acquire timeout, so an unreachable host fails fast with a clear message.
const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(6);

/// Fail fast (and legibly) when the DB host isn't reachable at all — the common case being an
/// RDS instance in a private subnet you can only reach over the VPN, or a security group that
/// doesn't allow your IP. Without this, an unreachable host silently burns the full 20s pool
/// acquire timeout and surfaces as an opaque "pool timed out".
async fn preflight(host: &str, port: u16) -> AppResult<()> {
    let addr = format!("{host}:{port}");
    match tokio::time::timeout(PREFLIGHT_TIMEOUT, tokio::net::TcpStream::connect(&addr)).await {
        Ok(Ok(_stream)) => Ok(()),
        Ok(Err(e)) => Err(AppError::Db(format!(
            "cannot reach {addr}: {e} — check the host and port are correct"
        ))),
        Err(_) => Err(AppError::Db(format!(
            "cannot reach {addr} within {}s — is the instance reachable from here? \
             Check that you're on the VPN and its security group allows your IP",
            PREFLIGHT_TIMEOUT.as_secs()
        ))),
    }
}

/// Open (or replace) a connection for `meta`, storing it in state keyed by `meta.id`.
pub async fn connect(state: &AppState, meta: ConnectionMeta, sso_region: &str) -> AppResult<()> {
    // Bring up an SSH tunnel first if configured; the DB socket then targets the local forwarded
    // port. Otherwise connect directly, with a fast reachability preflight.
    let (target, tunnel) = match &meta.ssh {
        Some(ssh) => {
            let tunnel = crate::ssh::SshTunnel::open(ssh, &meta.host, meta.port).await?;
            let target = ConnectTarget {
                host: "127.0.0.1".into(),
                port: tunnel.local_port,
                verify_identity: false,
            };
            (target, Some(tunnel))
        }
        None => {
            // Reachability first: a clear, fast error beats a slow, opaque handshake timeout.
            preflight(&meta.host, meta.port).await?;
            let target = ConnectTarget {
                host: meta.host.clone(),
                port: meta.port,
                verify_identity: true,
            };
            (target, None)
        }
    };

    // build_pool validates the connection (surfacing the real TLS/auth error) before returning.
    let pool = match build_pool(state, &meta, &target, sso_region).await {
        Ok(pool) => pool,
        Err(e) => {
            // Don't leak the ssh child if the DB connection itself failed.
            if let Some(tunnel) = tunnel {
                tunnel.close().await;
            }
            return Err(e);
        }
    };

    let active = Arc::new(ActiveConnection {
        pool: tokio::sync::RwLock::new(pool),
        meta: meta.clone(),
        target,
        tunnel,
    });
    state
        .connections
        .write()
        .await
        .insert(meta.id.clone(), active);
    Ok(())
}

pub async fn disconnect(state: &AppState, id: &str) -> AppResult<()> {
    if let Some(active) = state.connections.write().await.remove(id) {
        active.pool.read().await.close().await;
        // The SSH tunnel (if any) is torn down when the last Arc to `active` drops — its ssh
        // child is spawned with kill_on_drop.
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

/// Rebuild the pool for an existing connection (transparent token refresh). Reuses the same
/// effective `target` (and the existing SSH tunnel, when present) — only the token/pool refresh.
async fn rebuild(state: &AppState, active: &ActiveConnection, sso_region: &str) -> AppResult<()> {
    let pool = build_pool(state, &active.meta, &active.target, sso_region).await?;
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

/// Pull the first column of each row as a String (for single-column introspection queries like
/// SHOW DATABASES / information_schema). MySQL commonly returns these text columns with a *binary*
/// collation, so a plain `String` decode fails — fall back to decoding the raw bytes as UTF-8.
/// (Without this fallback the schema browser silently comes up empty.)
fn first_col_strings(rows: &[MySqlRow]) -> Vec<String> {
    rows.iter()
        .filter_map(|r| {
            r.try_get::<String, _>(0).ok().or_else(|| {
                r.try_get::<Vec<u8>, _>(0)
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok())
            })
        })
        .collect()
}

/// Render a byte column for display: UTF-8 text when it decodes cleanly, else a `<N bytes TYPE>`
/// placeholder for genuinely binary data.
fn bytes_to_display(bytes: &[u8], ty: &str) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => format!("<{} bytes {}>", bytes.len(), ty),
    }
}

/// Render each column as an optional String for the UI grid. Numbers/dates are stringified;
/// binary columns are shown as UTF-8 text when valid (e.g. SHOW / information_schema text columns
/// arrive under a binary collation), otherwise a byte-count placeholder.
fn row_to_strings(row: &MySqlRow) -> Vec<Option<String>> {
    (0..row.len())
        .map(|i| {
            let raw = row.try_get_raw(i).ok();
            if raw.as_ref().map(|r| r.is_null()).unwrap_or(true) {
                return None;
            }
            let ty = raw
                .as_ref()
                .map(|r| r.type_info().name().to_string())
                .unwrap_or_default();
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
                Some(bytes_to_display(&v, &ty))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_errors_trigger_a_token_refresh_retry() {
        // These are the failure modes where the pool should rebuild with a fresh token and
        // retry once — an expired token shows up as "access denied", a dropped socket as
        // "broken pipe"/"connection reset", etc.
        for msg in [
            "connection reset by peer",
            "broken pipe",
            "Access denied for user 'x'@'y' (using password: YES)",
            "pool timed out while waiting for an open connection",
            "io error: unexpected end of file",
        ] {
            assert!(
                is_connection_error(&AppError::Db(msg.into())),
                "expected {msg:?} to be treated as a connection error"
            );
        }
    }

    #[test]
    fn query_errors_do_not_trigger_a_retry() {
        // A SQL/logic error must surface to the user immediately, not be masked by a retry.
        for msg in [
            "You have an error in your SQL syntax",
            "Unknown column 'foo' in 'field list'",
            "Table 'db.missing' doesn't exist",
        ] {
            assert!(
                !is_connection_error(&AppError::Db(msg.into())),
                "expected {msg:?} to NOT be treated as a connection error"
            );
        }
        // Non-Db variants are never connection errors.
        assert!(!is_connection_error(&AppError::NeedsLogin));
    }

    #[tokio::test]
    async fn preflight_reports_an_unreachable_host_clearly() {
        // Port 1 on loopback has nothing listening: the dial is refused immediately (fast,
        // deterministic, no network) and must surface our actionable "cannot reach" message
        // naming the exact host:port — not an opaque error.
        let err = preflight("127.0.0.1", 1).await.expect_err("should fail");
        let AppError::Db(msg) = err else {
            panic!("expected a Db error")
        };
        assert!(
            msg.contains("127.0.0.1:1"),
            "message names the target: {msg}"
        );
        assert!(msg.contains("cannot reach"), "message is actionable: {msg}");
    }

    // Integration test against a real MySQL. Ignored by default (CI has no DB); run with:
    //   RDSCONNECT_TEST_DSN='mysql://root:testpw@127.0.0.1:3307' cargo test -- --ignored
    // Proves the binary-collation decode fix end to end: SHOW DATABASES / information_schema
    // text columns come back under a binary collation and must render as readable text.
    #[tokio::test]
    #[ignore = "requires a MySQL server; set RDSCONNECT_TEST_DSN"]
    async fn decodes_binary_collation_text_from_a_real_mysql() {
        let dsn = std::env::var("RDSCONNECT_TEST_DSN").expect("set RDSCONNECT_TEST_DSN");
        let mut conn = sqlx::mysql::MySqlConnection::connect(&dsn)
            .await
            .expect("connect to test MySQL");

        // SHOW DATABASES returns the `Database` column under a binary collation.
        let rows = sqlx::query("SHOW DATABASES")
            .fetch_all(&mut conn)
            .await
            .expect("SHOW DATABASES");

        // first_col_strings (schema browser) must surface real names, not drop them.
        let names = first_col_strings(&rows);
        assert!(
            names.iter().any(|n| n == "information_schema"),
            "schema list should contain information_schema, got {names:?}"
        );
        assert!(
            names.iter().any(|n| n == "mysql"),
            "schema list should contain mysql, got {names:?}"
        );

        // row_to_strings (results grid) must render text, never "<N bytes VARBINARY>".
        let grid: Vec<Vec<Option<String>>> = rows.iter().map(row_to_strings).collect();
        let cells: Vec<&String> = grid.iter().flatten().flatten().collect();
        assert!(
            cells.iter().any(|s| *s == "information_schema"),
            "grid should render db names as text, got {cells:?}"
        );
        assert!(
            !cells.iter().any(|s| s.contains("VARBINARY")),
            "grid should not show binary placeholders, got {cells:?}"
        );

        let _ = conn.close().await;
    }

    #[test]
    fn bytes_to_display_renders_utf8_text_but_placeholders_true_binary() {
        // Text columns returned under a binary collation (SHOW DATABASES, information_schema)
        // must render as their actual text, not "<N bytes VARBINARY>".
        assert_eq!(
            bytes_to_display(b"information_schema", "VARBINARY"),
            "information_schema"
        );
        assert_eq!(bytes_to_display("héllo".as_bytes(), "BLOB"), "héllo");
        assert_eq!(bytes_to_display(b"", "VARBINARY"), "");
        // Genuinely non-UTF-8 bytes fall back to the size/type placeholder.
        assert_eq!(
            bytes_to_display(&[0xff, 0xfe, 0x00, 0x01], "BLOB"),
            "<4 bytes BLOB>"
        );
    }

    #[tokio::test]
    async fn validate_connection_surfaces_the_real_error_not_a_pool_timeout() {
        // Dialing a refused local port must yield the actual "connect: ..." error immediately,
        // proving we bypass the pool's generic "pool timed out" masking. (Regression guard for
        // the whole reason this connection path was rewritten.)
        let opts = MySqlConnectOptions::new()
            .host("127.0.0.1")
            .port(1)
            .username("x");
        let err = validate_connection(&opts).await.expect_err("should fail");
        let AppError::Db(msg) = err else {
            panic!("expected a Db error")
        };
        assert!(
            msg.starts_with("connect:"),
            "surfaces the real connect error: {msg}"
        );
        assert!(
            !msg.contains("pool timed out"),
            "must NOT be the masked pool error: {msg}"
        );
    }
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
