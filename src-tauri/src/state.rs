// Shared application state + error type.
//
// AppState holds everything the UI must never see directly: the SSO access token,
// per-role temporary credentials, and live DB connection pools. Everything is behind
// async-aware locks so Tauri commands (which may run concurrently) stay safe.

use std::collections::HashMap;
use std::sync::Arc;

use aws_credential_types::Credentials;
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use tokio::sync::RwLock;

/// One error type surfaced to the UI. `Display` is what the user sees, so keep messages
/// human-readable and free of secrets.
#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("SSO login required")]
    NeedsLogin,
    #[error("SSO error: {0}")]
    Sso(String),
    #[error("AWS error: {0}")]
    Aws(String),
    #[error("token generation failed: {0}")]
    Token(String),
    #[error("database error: {0}")]
    Db(String),
    #[error("no active connection for '{0}'")]
    NoConnection(String),
    #[error("persistence error: {0}")]
    Persistence(String),
}

// Tauri commands must return `Result<T, E: Serialize>`. Serialize the message.
impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

/// Cached SSO access token (from the device-auth flow or the ~/.aws/sso cache).
#[derive(Clone, Debug)]
pub struct SsoToken {
    pub access_token: String,
    /// Unix seconds when the token expires.
    pub expires_at: i64,
}

impl SsoToken {
    pub fn is_valid(&self) -> bool {
        // 60s of slack so we don't hand out a token that dies mid-request.
        self.expires_at - 60 > chrono::Utc::now().timestamp()
    }
}

/// Temporary role credentials from `get-role-credentials`, keyed by (account, role).
#[derive(Clone, Debug)]
pub struct RoleCreds {
    pub credentials: Credentials,
    /// Unix ms when the creds expire (SSO reports ms).
    pub expiration_ms: i64,
}

impl RoleCreds {
    pub fn is_valid(&self) -> bool {
        self.expiration_ms - 60_000 > chrono::Utc::now().timestamp_millis()
    }
}

/// A live DB connection pool plus the info needed to regenerate its auth token on reconnect.
/// The pool sits behind a lock so `db::rebuild` can swap in a fresh one (with a new token)
/// after the 15-minute expiry without the UI noticing.
pub struct ActiveConnection {
    pub pool: RwLock<MySqlPool>,
    pub meta: ConnectionMeta,
}

/// Everything needed to (re)build an auth token + connection. Mirrors a saved connection,
/// minus any secret.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionMeta {
    pub id: String,
    pub label: String,
    pub account_id: String,
    pub role_name: String,
    pub region: String,
    pub instance_id: String,
    pub host: String,
    pub port: u16,
    pub db_user: String,
    #[serde(default)]
    pub default_database: Option<String>,
}

#[derive(Default)]
pub struct AppState {
    pub sso_token: RwLock<Option<SsoToken>>,
    pub role_creds: RwLock<HashMap<String, RoleCreds>>,
    pub connections: RwLock<HashMap<String, Arc<ActiveConnection>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cache key for a set of role credentials.
    pub fn cred_key(account_id: &str, role_name: &str) -> String {
        format!("{account_id}/{role_name}")
    }
}
