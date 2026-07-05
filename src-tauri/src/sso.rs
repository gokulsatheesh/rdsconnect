// SSO login: native device-authorization flow, with reuse of an existing cached token.
//
// register_client -> start_device_authorization -> open the browser to verificationUriComplete
// -> poll create_token until the user approves. The resulting access token is cached to
// ~/.config/rdsconnect/sso.json and reused until it expires. If the AWS CLI has already been
// used to log in, its ~/.aws/sso/cache token is picked up as a fallback on startup.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::connections::config_dir;
use crate::state::{AppError, AppResult, AppState, SsoToken};

const CLIENT_NAME: &str = "rdsconnect";

/// Emitted to the UI while waiting for the user to approve the device-authorization request.
#[derive(Clone, Serialize)]
struct DeviceCode {
    user_code: String,
    verification_uri: String,
}

/// Persisted SSO token cache (our own, not AWS CLI's).
#[derive(Serialize, Deserialize)]
struct CachedToken {
    access_token: String,
    expires_at: i64,
    start_url: String,
}

fn sso_cache_path() -> AppResult<PathBuf> {
    Ok(config_dir()?.join("sso.json"))
}

/// Load a valid SSO token into state without prompting, if one is available.
/// Checks (1) our own cache, then (2) the AWS CLI cache as a fallback.
pub async fn try_restore(state: &AppState, start_url: &str) -> AppResult<bool> {
    if let Some(tok) = load_cached(start_url).await? {
        if tok.is_valid() {
            *state.sso_token.write().await = Some(tok);
            return Ok(true);
        }
    }
    if let Some(tok) = load_from_aws_cli_cache().await? {
        if tok.is_valid() {
            *state.sso_token.write().await = Some(tok);
            return Ok(true);
        }
    }
    Ok(false)
}

async fn load_cached(start_url: &str) -> AppResult<Option<SsoToken>> {
    let path = sso_cache_path()?;
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return Ok(None);
    };
    let cached: CachedToken =
        serde_json::from_slice(&bytes).map_err(|e| AppError::Sso(e.to_string()))?;
    if cached.start_url != start_url {
        return Ok(None);
    }
    Ok(Some(SsoToken {
        access_token: cached.access_token,
        expires_at: cached.expires_at,
    }))
}

/// Fallback: newest token in ~/.aws/sso/cache/*.json, like the original script.
async fn load_from_aws_cli_cache() -> AppResult<Option<SsoToken>> {
    let Some(home) = directories::UserDirs::new().map(|d| d.home_dir().to_path_buf()) else {
        return Ok(None);
    };
    let dir = home.join(".aws/sso/cache");
    let mut entries = match tokio::fs::read_dir(&dir).await {
        Ok(e) => e,
        Err(_) => return Ok(None),
    };

    let mut newest: Option<(std::time::SystemTime, SsoToken)> = None;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(bytes) = tokio::fs::read(&path).await else {
            continue;
        };
        // AWS CLI cache entries: { "accessToken": "...", "expiresAt": "2024-...Z", ... }
        #[derive(Deserialize)]
        struct AwsCacheEntry {
            #[serde(rename = "accessToken")]
            access_token: Option<String>,
            #[serde(rename = "expiresAt")]
            expires_at: Option<String>,
        }
        let Ok(parsed) = serde_json::from_slice::<AwsCacheEntry>(&bytes) else {
            continue;
        };
        let (Some(token), Some(exp)) = (parsed.access_token, parsed.expires_at) else {
            continue;
        };
        let expires_at = chrono::DateTime::parse_from_rfc3339(&exp)
            .map(|d| d.timestamp())
            .unwrap_or(0);
        let mtime = entry
            .metadata()
            .await
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        let candidate = SsoToken {
            access_token: token,
            expires_at,
        };
        if newest.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
            newest = Some((mtime, candidate));
        }
    }
    Ok(newest.map(|(_, t)| t))
}

/// Run the full device-authorization login and cache the resulting token in state + disk.
/// Emits an `sso-device-code` event so the UI can show the code the user must confirm.
pub async fn login(
    app: &tauri::AppHandle,
    state: &AppState,
    start_url: &str,
    region: &str,
) -> AppResult<SsoToken> {
    if start_url.trim().is_empty() {
        return Err(AppError::Sso(
            "Set your AWS SSO start URL in Settings first.".into(),
        ));
    }

    let cfg = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_types::region::Region::new(region.to_string()))
        .load()
        .await;
    let oidc = aws_sdk_ssooidc::Client::new(&cfg);

    // 1. Register this client (public client, no secret needed for device grant).
    let reg = oidc
        .register_client()
        .client_name(CLIENT_NAME)
        .client_type("public")
        .send()
        .await
        .map_err(|e| AppError::Sso(format!("register_client: {e}")))?;
    let client_id = reg.client_id().unwrap_or_default().to_string();
    let client_secret = reg.client_secret().unwrap_or_default().to_string();

    // 2. Start device authorization.
    let dev = oidc
        .start_device_authorization()
        .client_id(&client_id)
        .client_secret(&client_secret)
        .start_url(start_url)
        .send()
        .await
        .map_err(|e| AppError::Sso(format!("start_device_authorization: {e}")))?;

    let device_code = dev.device_code().unwrap_or_default().to_string();
    let user_code = dev.user_code().unwrap_or_default().to_string();
    let verification_uri = dev.verification_uri().unwrap_or_default().to_string();
    let verification_uri_complete = dev
        .verification_uri_complete()
        .or(dev.verification_uri())
        .unwrap_or_default()
        .to_string();
    let interval = dev.interval().max(1) as u64;
    let expires_in = dev.expires_in();

    // 3. Tell the UI the code to confirm, then open the browser for approval.
    let _ = app.emit(
        "sso-device-code",
        DeviceCode {
            user_code,
            verification_uri,
        },
    );
    let _ = open::that(&verification_uri_complete);

    // 4. Poll create_token until approval, denial, or timeout.
    let deadline = std::time::Instant::now() + Duration::from_secs(expires_in.max(60) as u64);
    loop {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        if std::time::Instant::now() > deadline {
            return Err(AppError::Sso("device authorization timed out".into()));
        }
        match oidc
            .create_token()
            .client_id(&client_id)
            .client_secret(&client_secret)
            .grant_type("urn:ietf:params:oauth:grant-type:device_code")
            .device_code(&device_code)
            .send()
            .await
        {
            Ok(tok) => {
                let access_token = tok
                    .access_token()
                    .ok_or_else(|| AppError::Sso("no access token in response".into()))?
                    .to_string();
                let expires_at = chrono::Utc::now().timestamp() + tok.expires_in() as i64;
                let sso = SsoToken {
                    access_token,
                    expires_at,
                };
                persist(&sso, start_url).await?;
                *state.sso_token.write().await = Some(sso.clone());
                return Ok(sso);
            }
            Err(err) => {
                // The SDK reports "still waiting" / "poll slower" as typed service errors;
                // their Display string is just "service error", so we must match the variant.
                if let Some(svc) = err.as_service_error() {
                    if svc.is_authorization_pending_exception() {
                        continue;
                    }
                    if svc.is_slow_down_exception() {
                        tokio::time::sleep(Duration::from_secs(interval + 5)).await;
                        continue;
                    }
                }
                return Err(AppError::Sso(format!("create_token: {err}")));
            }
        }
    }
}

async fn persist(tok: &SsoToken, start_url: &str) -> AppResult<()> {
    let cached = CachedToken {
        access_token: tok.access_token.clone(),
        expires_at: tok.expires_at,
        start_url: start_url.to_string(),
    };
    let path = sso_cache_path()?;
    let bytes = serde_json::to_vec_pretty(&cached).map_err(|e| AppError::Sso(e.to_string()))?;
    tokio::fs::write(&path, bytes)
        .await
        .map_err(|e| AppError::Sso(e.to_string()))?;
    // The cached SSO access token is a bearer secret — restrict it to the owner (0600).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .await
            .map_err(|e| AppError::Sso(e.to_string()))?;
    }
    Ok(())
}

/// Return the current SSO access token, erroring if login is required.
pub async fn require_token(state: &AppState) -> AppResult<String> {
    let guard = state.sso_token.read().await;
    match guard.as_ref() {
        Some(t) if t.is_valid() => Ok(t.access_token.clone()),
        _ => Err(AppError::NeedsLogin),
    }
}
