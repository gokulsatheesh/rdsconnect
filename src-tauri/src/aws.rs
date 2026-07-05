// AWS discovery: accounts, roles, temporary credentials, and RDS instances.

use aws_credential_types::Credentials;
use serde::Serialize;

use crate::sso::require_token;
use crate::state::{AppError, AppResult, AppState, RoleCreds};

#[derive(Serialize, Clone)]
pub struct Account {
    pub account_id: String,
    pub account_name: String,
    pub email: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct DbInstance {
    pub identifier: String,
    pub host: String,
    pub port: u16,
    pub engine: String,
    pub region: String,
}

fn sso_client(sso_region: &str) -> aws_sdk_sso::Client {
    // The SSO portal APIs run in the SSO region (the region used at login).
    let conf = aws_sdk_sso::Config::builder()
        .behavior_version(aws_sdk_sso::config::BehaviorVersion::latest())
        .region(aws_sdk_sso::config::Region::new(sso_region.to_string()))
        .build();
    aws_sdk_sso::Client::from_conf(conf)
}

/// List the AWS accounts the SSO session can access, sorted by name.
pub async fn list_accounts(state: &AppState, sso_region: &str) -> AppResult<Vec<Account>> {
    let token = require_token(state).await?;
    let client = sso_client(sso_region);

    let mut out = Vec::new();
    let mut stream = client
        .list_accounts()
        .access_token(&token)
        .into_paginator()
        .send();
    while let Some(page) = stream.next().await {
        let page = page.map_err(|e| AppError::Aws(format!("list_accounts: {e}")))?;
        for a in page.account_list() {
            out.push(Account {
                account_id: a.account_id().unwrap_or_default().to_string(),
                account_name: a.account_name().unwrap_or_default().to_string(),
                email: a.email_address().map(|s| s.to_string()),
            });
        }
    }
    out.sort_by(|a, b| {
        a.account_name
            .to_lowercase()
            .cmp(&b.account_name.to_lowercase())
    });
    Ok(out)
}

/// list-account-roles, sorted by name.
pub async fn list_account_roles(
    state: &AppState,
    sso_region: &str,
    account_id: &str,
) -> AppResult<Vec<String>> {
    let token = require_token(state).await?;
    let client = sso_client(sso_region);

    let mut out = Vec::new();
    let mut stream = client
        .list_account_roles()
        .access_token(&token)
        .account_id(account_id)
        .into_paginator()
        .send();
    while let Some(page) = stream.next().await {
        let page = page.map_err(|e| AppError::Aws(format!("list_account_roles: {e}")))?;
        for r in page.role_list() {
            if let Some(name) = r.role_name() {
                out.push(name.to_string());
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Get temporary role credentials, caching them by (account, role) until expiry.
pub async fn get_role_credentials(
    state: &AppState,
    sso_region: &str,
    account_id: &str,
    role_name: &str,
) -> AppResult<Credentials> {
    let key = AppState::cred_key(account_id, role_name);

    // Reuse cached creds while valid.
    if let Some(c) = state.role_creds.read().await.get(&key) {
        if c.is_valid() {
            return Ok(c.credentials.clone());
        }
    }

    let token = require_token(state).await?;
    let client = sso_client(sso_region);
    let resp = client
        .get_role_credentials()
        .access_token(&token)
        .account_id(account_id)
        .role_name(role_name)
        .send()
        .await
        .map_err(|e| AppError::Aws(format!("get_role_credentials: {e}")))?;

    let rc = resp
        .role_credentials()
        .ok_or_else(|| AppError::Aws("no roleCredentials in response".into()))?;

    let creds = Credentials::new(
        rc.access_key_id().unwrap_or_default(),
        rc.secret_access_key().unwrap_or_default(),
        rc.session_token().map(|s| s.to_string()),
        None,
        "rdsconnect-sso",
    );

    state.role_creds.write().await.insert(
        key,
        RoleCreds {
            credentials: creds.clone(),
            expiration_ms: rc.expiration(),
        },
    );
    Ok(creds)
}

/// describe-db-instances, filtered to MySQL-family engines. Requires role creds for the account.
pub async fn describe_db_instances(
    state: &AppState,
    sso_region: &str,
    account_id: &str,
    role_name: &str,
    region: &str,
) -> AppResult<Vec<DbInstance>> {
    let creds = get_role_credentials(state, sso_region, account_id, role_name).await?;

    let conf = aws_sdk_rds::Config::builder()
        .behavior_version(aws_sdk_rds::config::BehaviorVersion::latest())
        .region(aws_sdk_rds::config::Region::new(region.to_string()))
        .credentials_provider(creds)
        .build();
    let client = aws_sdk_rds::Client::from_conf(conf);

    let mut out = Vec::new();
    let mut stream = client.describe_db_instances().into_paginator().send();
    while let Some(page) = stream.next().await {
        let page = page.map_err(|e| AppError::Aws(format!("describe_db_instances: {e}")))?;
        for inst in page.db_instances() {
            let engine = inst.engine().unwrap_or_default().to_string();
            if !matches!(engine.as_str(), "mysql" | "aurora-mysql") {
                continue;
            }
            let Some(endpoint) = inst.endpoint() else {
                continue;
            };
            out.push(DbInstance {
                identifier: inst
                    .db_instance_identifier()
                    .unwrap_or_default()
                    .to_string(),
                host: endpoint.address().unwrap_or_default().to_string(),
                port: endpoint.port().unwrap_or(3306) as u16,
                engine,
                region: region.to_string(),
            });
        }
    }
    out.sort_by(|a, b| a.identifier.cmp(&b.identifier));
    Ok(out)
}
