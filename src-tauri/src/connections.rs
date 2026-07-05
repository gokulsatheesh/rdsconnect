// Persistence: saved connections, query history, and settings — all under ~/.config/rdsconnect/.
// NO SECRETS are ever written here; auth always flows live through SSO.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::state::{AppError, AppResult, ConnectionMeta};

/// ~/.config/rdsconnect/, created on first use.
pub fn config_dir() -> AppResult<PathBuf> {
    let dirs = directories::ProjectDirs::from("io.github", "gokulsatheesh", "rdsconnect")
        .ok_or_else(|| AppError::Persistence("could not resolve config dir".into()))?;
    let dir = dirs.config_dir().to_path_buf();
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
    if all.len() > HISTORY_CAP {
        let overflow = all.len() - HISTORY_CAP;
        all.drain(0..overflow);
    }
    write_json(history_path()?, &all).await
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
