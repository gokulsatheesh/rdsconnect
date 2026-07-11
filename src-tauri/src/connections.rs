// Persistence: saved connections, query history, and settings — all under ~/.config/rdsconnect/.
// NO SECRETS are ever written here; auth always flows live through SSO.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::state::{AppError, AppResult, ConnectionMeta};

/// ~/.config/rdsconnect/, created on first use. `RDSCONNECT_CONFIG_DIR` overrides the location
/// (used by tests to isolate state, and handy for relocating config), otherwise the platform
/// config dir is used.
pub fn config_dir() -> AppResult<PathBuf> {
    let dir = if let Some(override_dir) = std::env::var_os("RDSCONNECT_CONFIG_DIR") {
        PathBuf::from(override_dir)
    } else {
        directories::ProjectDirs::from("io.github", "gokulsatheesh", "rdsconnect")
            .ok_or_else(|| AppError::Persistence("could not resolve config dir".into()))?
            .config_dir()
            .to_path_buf()
    };
    std::fs::create_dir_all(&dir).map_err(|e| AppError::Persistence(e.to_string()))?;
    Ok(dir)
}

fn connections_path() -> AppResult<PathBuf> {
    Ok(config_dir()?.join("connections.json"))
}
fn history_path() -> AppResult<PathBuf> {
    Ok(config_dir()?.join("history.json"))
}
fn settings_path() -> AppResult<PathBuf> {
    Ok(config_dir()?.join("settings.json"))
}

// ---- Settings ----

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub sso_start_url: String,
    pub sso_region: String,
    pub default_db_user: String,
    pub default_rds_region: String,
}

impl Default for Settings {
    fn default() -> Self {
        // Empty SSO URL on first run — the user sets their org's start URL in Settings.
        // Regions default to a common AWS region; the DB user defaults to the OS username.
        Self {
            sso_start_url: String::new(),
            sso_region: "us-east-1".into(),
            default_db_user: whoami_default(),
            default_rds_region: "us-east-1".into(),
        }
    }
}

fn whoami_default() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "admin".into())
}

pub async fn load_settings() -> AppResult<Settings> {
    let path = settings_path()?;
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|e| AppError::Persistence(e.to_string()))
        }
        Err(_) => Ok(Settings::default()),
    }
}

pub async fn save_settings(s: &Settings) -> AppResult<()> {
    write_json(settings_path()?, s).await
}

// ---- Saved connections ----

pub async fn load_connections() -> AppResult<Vec<ConnectionMeta>> {
    read_json_or_default(connections_path()?).await
}

pub async fn save_connection(meta: ConnectionMeta) -> AppResult<Vec<ConnectionMeta>> {
    let mut all = load_connections().await?;
    match all.iter_mut().find(|c| c.id == meta.id) {
        Some(existing) => *existing = meta,
        None => all.push(meta),
    }
    write_json(connections_path()?, &all).await?;
    Ok(all)
}

pub async fn delete_connection(id: &str) -> AppResult<Vec<ConnectionMeta>> {
    let mut all = load_connections().await?;
    all.retain(|c| c.id != id);
    write_json(connections_path()?, &all).await?;
    Ok(all)
}

// ---- Query history ----

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HistoryEntry {
    pub connection_id: String,
    pub sql: String,
    /// RFC3339 timestamp.
    pub at: String,
    pub rows: u64,
    pub elapsed_ms: u128,
    pub ok: bool,
}

const HISTORY_CAP: usize = 500;

pub async fn load_history() -> AppResult<Vec<HistoryEntry>> {
    read_json_or_default(history_path()?).await
}

pub async fn append_history(entry: HistoryEntry) -> AppResult<()> {
    let mut all = load_history().await?;
    all.push(entry);
    cap_history(&mut all, HISTORY_CAP);
    write_json(history_path()?, &all).await
}

/// Trim `all` in place to at most `cap` entries, dropping the oldest (front) first so the most
/// recent queries are always kept.
fn cap_history(all: &mut Vec<HistoryEntry>, cap: usize) {
    if all.len() > cap {
        let overflow = all.len() - cap;
        all.drain(0..overflow);
    }
}

// ---- helpers ----

async fn read_json_or_default<T: for<'de> Deserialize<'de> + Default>(
    path: PathBuf,
) -> AppResult<T> {
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|e| AppError::Persistence(e.to_string()))
        }
        Err(_) => Ok(T::default()),
    }
}

async fn write_json<T: Serialize>(path: PathBuf, value: &T) -> AppResult<()> {
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|e| AppError::Persistence(e.to_string()))?;
    tokio::fs::write(&path, bytes)
        .await
        .map_err(|e| AppError::Persistence(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(sql: &str) -> HistoryEntry {
        HistoryEntry {
            connection_id: "c".into(),
            sql: sql.into(),
            at: "2026-07-06T00:00:00Z".into(),
            rows: 0,
            elapsed_ms: 0,
            ok: true,
        }
    }

    #[test]
    fn cap_history_keeps_the_newest_and_drops_the_oldest() {
        let mut h: Vec<_> = (0..5).map(|i| entry(&format!("q{i}"))).collect();
        cap_history(&mut h, 3);
        assert_eq!(h.len(), 3);
        // Oldest (q0, q1) dropped; newest kept in order.
        assert_eq!(h[0].sql, "q2");
        assert_eq!(h[2].sql, "q4");
    }

    #[test]
    fn cap_history_leaves_under_cap_untouched() {
        let mut h = vec![entry("a"), entry("b")];
        cap_history(&mut h, 10);
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn settings_default_has_empty_sso_url_and_sane_regions() {
        // First-run defaults: no SSO URL (user must set it), regions default to us-east-1.
        let s = Settings::default();
        assert!(s.sso_start_url.is_empty());
        assert_eq!(s.sso_region, "us-east-1");
        assert_eq!(s.default_rds_region, "us-east-1");
        assert!(!s.default_db_user.is_empty());
    }

    fn meta(id: &str) -> ConnectionMeta {
        ConnectionMeta {
            id: id.into(),
            label: "l".into(),
            account_id: "123456789012".into(),
            role_name: "r".into(),
            region: "us-east-1".into(),
            instance_id: "db-1".into(),
            host: "db.example.rds.amazonaws.com".into(),
            port: 3306,
            db_user: "gokul".into(),
            default_database: None,
        }
    }

    // One serial test drives every persistence path against an isolated temp dir via the
    // RDSCONNECT_CONFIG_DIR override — env is process-global, so keeping it to a single test
    // avoids cross-test races.
    #[tokio::test]
    async fn persistence_round_trips_against_an_isolated_dir() {
        let dir = std::env::temp_dir().join(format!("rdsconnect-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("RDSCONNECT_CONFIG_DIR", &dir);

        // Connections: empty → save two → update one → delete one.
        assert!(load_connections().await.unwrap().is_empty());
        save_connection(meta("a")).await.unwrap();
        let all = save_connection(meta("b")).await.unwrap();
        assert_eq!(all.len(), 2);

        let mut updated = meta("a");
        updated.label = "renamed".into();
        let all = save_connection(updated).await.unwrap();
        assert_eq!(all.len(), 2, "same id updates in place, not appends");
        assert_eq!(all.iter().find(|c| c.id == "a").unwrap().label, "renamed");

        let all = delete_connection("a").await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "b");

        // Settings: save → load reflects it.
        let s = Settings {
            sso_start_url: "https://example.awsapps.com/start".into(),
            ..Settings::default()
        };
        save_settings(&s).await.unwrap();
        assert_eq!(
            load_settings().await.unwrap().sso_start_url,
            "https://example.awsapps.com/start"
        );

        // History: append past the cap keeps only the newest HISTORY_CAP.
        for i in 0..(HISTORY_CAP + 10) {
            append_history(entry(&format!("q{i}"))).await.unwrap();
        }
        let h = load_history().await.unwrap();
        assert_eq!(h.len(), HISTORY_CAP);
        assert_eq!(h.last().unwrap().sql, format!("q{}", HISTORY_CAP + 9));

        std::env::remove_var("RDSCONNECT_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
