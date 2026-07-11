// Shared application state + error type.
//
// AppState holds everything the UI must never see directly: the SSO access token,
// per-role temporary credentials, and live DB connection pools. Everything is behind
// async-aware locks so Tauri commands (which may run concurrently) stay safe.

use std::collections::HashMap;
use std::path::PathBuf;
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
    /// Resolved at startup from the Tauri resource dir (falls back to `RDSCONNECT_RDS_CA` or the
    /// in-repo path in dev). `None` means it couldn't be found — surfaced as an error on connect.
    pub ca_bundle_path: Option<PathBuf>,
}

impl AppState {
    pub fn new(ca_bundle_path: Option<PathBuf>) -> Self {
        Self {
            ca_bundle_path,
            ..Self::default()
        }
    }

    /// Cache key for a set of role credentials.
    pub fn cred_key(account_id: &str, role_name: &str) -> String {
        format!("{account_id}/{role_name}")
    }
}

/// Locate the bundled RDS CA at startup: an explicit override first, then Tauri's resolved
/// resource dir (correct for an installed .deb/.AppImage regardless of where it was built),
/// then the in-repo path for `cargo run`/`tauri dev`.
pub fn resolve_ca_bundle_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;

    if let Ok(p) = std::env::var("RDSCONNECT_RDS_CA") {
        return Some(PathBuf::from(p));
    }
    if let Ok(p) = app.path().resolve(
        "resources/global-bundle.pem",
        tauri::path::BaseDirectory::Resource,
    ) {
        if p.exists() {
            return Some(p);
        }
    }
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/global-bundle.pem");
    dev.exists().then_some(dev)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sso_token_validity_respects_the_60s_slack() {
        let now = chrono::Utc::now().timestamp();
        // Expires comfortably in the future → valid.
        let good = SsoToken {
            access_token: "t".into(),
            expires_at: now + 3600,
        };
        assert!(good.is_valid());
        // Already expired → invalid.
        let expired = SsoToken {
            access_token: "t".into(),
            expires_at: now - 10,
        };
        assert!(!expired.is_valid());
        // Inside the 60s slack window → treated as invalid so we don't hand out a dying token.
        let almost = SsoToken {
            access_token: "t".into(),
            expires_at: now + 30,
        };
        assert!(!almost.is_valid());
    }

    #[test]
    fn role_creds_validity_uses_millisecond_slack() {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let good = RoleCreds {
            credentials: Credentials::new("ak", "sk", None, None, "t"),
            expiration_ms: now_ms + 600_000,
        };
        assert!(good.is_valid());
        let expired = RoleCreds {
            credentials: Credentials::new("ak", "sk", None, None, "t"),
            expiration_ms: now_ms - 1,
        };
        assert!(!expired.is_valid());
    }

    #[test]
    fn cred_key_is_stable_and_scoped_by_account_and_role() {
        assert_eq!(AppState::cred_key("123", "Admin"), "123/Admin");
        assert_ne!(
            AppState::cred_key("123", "Admin"),
            AppState::cred_key("123", "ReadOnly")
        );
    }
}
