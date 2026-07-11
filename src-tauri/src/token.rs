// The RDS IAM auth token — the native replacement for `aws rds generate-db-auth-token`.
//
// The token is a SigV4 *query-presigned* URL for:
//     GET https://{host}:{port}/?Action=connect&DBUser={db_user}
// signed against service `rds-db` in the DB's region with 900s expiry, with the
// `https://` scheme stripped off. The result is used verbatim as the MySQL password.
//
// Parity target: structurally identical to the CLI's output
//   {host}:{port}/?Action=connect&DBUser=...&X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=...
//   &X-Amz-Date=...&X-Amz-Expires=900&X-Amz-SignedHeaders=host&X-Amz-Security-Token=...&X-Amz-Signature=...

use std::time::{Duration, SystemTime};

use aws_credential_types::Credentials;
use aws_sigv4::http_request::{
    sign, SignableBody, SignableRequest, SignatureLocation, SigningSettings,
};
use aws_sigv4::sign::v4;

use crate::state::{AppError, AppResult};

const SERVICE: &str = "rds-db";
const EXPIRY_SECS: u64 = 900; // 15 minutes

/// Generate an RDS auth token for `db_user` against `host:port` in `region`.
pub fn generate(
    creds: &Credentials,
    region: &str,
    host: &str,
    port: u16,
    db_user: &str,
) -> AppResult<String> {
    let mut settings = SigningSettings::default();
    settings.signature_location = SignatureLocation::QueryParams;
    settings.expires_in = Some(Duration::from_secs(EXPIRY_SECS));

    let identity = creds.clone().into();
    let params = v4::SigningParams::builder()
        .identity(&identity)
        .region(region)
        .name(SERVICE)
        .time(SystemTime::now())
        .settings(settings)
        .build()
        .map_err(|e| AppError::Token(format!("signing params: {e}")))?
        .into();

    let url = format!(
        "https://{host}:{port}/?Action=connect&DBUser={}",
        urlencode(db_user)
    );

    let signable = SignableRequest::new("GET", &url, std::iter::empty(), SignableBody::Bytes(&[]))
        .map_err(|e| AppError::Token(format!("signable request: {e}")))?;

    let (instructions, _sig) = sign(signable, &params)
        .map_err(|e| AppError::Token(format!("sign: {e}")))?
        .into_parts();

    // Apply the query-string additions the signer produced to our URL.
    let mut req = http::Request::builder()
        .method("GET")
        .uri(&url)
        .body(())
        .map_err(|e| AppError::Token(e.to_string()))?;
    instructions.apply_to_request_http1x(&mut req);

    // Strip the scheme; the token is `host:port/?...`.
    let signed = req.uri().to_string();
    Ok(signed
        .strip_prefix("https://")
        .unwrap_or(&signed)
        .to_string())
}

/// Minimal RFC 3986 encoding for the DBUser value (usernames are simple, but be safe).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creds() -> Credentials {
        Credentials::new(
            "AKIDEXAMPLE",
            "secret",
            Some("session".into()),
            None,
            "test",
        )
    }

    #[test]
    fn token_has_expected_structure() {
        let t = generate(
            &creds(),
            "us-east-1",
            "db.example.rds.amazonaws.com",
            3306,
            "gokul",
        )
        .expect("token");
        // No scheme, correct host:port, and the RDS-specific query params.
        assert!(!t.starts_with("https://"));
        assert!(t.starts_with("db.example.rds.amazonaws.com:3306/?"));
        assert!(t.contains("Action=connect"));
        assert!(t.contains("DBUser=gokul"));
        assert!(t.contains("X-Amz-Algorithm=AWS4-HMAC-SHA256"));
        assert!(t.contains("X-Amz-Expires=900"));
        assert!(t.contains("X-Amz-Security-Token=")); // session token present
        assert!(t.contains("X-Amz-Signature="));
    }

    #[test]
    fn token_scope_carries_the_db_region_and_service() {
        // The signing region must be the DB's region (not the SSO/login region) or RDS rejects
        // the token. The credential scope encodes it as `.../<region>/rds-db/aws4_request`.
        let t = generate(
            &creds(),
            "eu-west-2",
            "db.example.rds.amazonaws.com",
            3306,
            "gokul",
        )
        .expect("token");
        assert!(
            t.contains("eu-west-2%2Frds-db") || t.contains("eu-west-2/rds-db"),
            "credential scope should carry region + rds-db service: {t}"
        );
    }

    #[test]
    fn token_without_session_token_omits_the_security_token_param() {
        // Long-lived IAM users have no session token; the presigned URL must then omit it.
        let creds = Credentials::new("AKIDEXAMPLE", "secret", None, None, "test");
        let t = generate(
            &creds,
            "us-east-1",
            "db.example.rds.amazonaws.com",
            3306,
            "gokul",
        )
        .expect("token");
        assert!(!t.contains("X-Amz-Security-Token="));
    }

    #[test]
    fn urlencode_percent_encodes_reserved_characters() {
        // Usernames are usually simple, but IAM allows a few punctuation chars; anything outside
        // the RFC3986 unreserved set must be percent-encoded so the query param stays well-formed.
        assert_eq!(urlencode("gokul.satheesh"), "gokul.satheesh"); // unreserved, untouched
        assert_eq!(urlencode("a+b c"), "a%2Bb%20c");
        assert_eq!(urlencode("user@host"), "user%40host");
        assert_eq!(urlencode("a/b"), "a%2Fb");
    }

    #[test]
    fn token_encodes_special_characters_in_the_db_user() {
        let t = generate(
            &creds(),
            "us-east-1",
            "db.example.rds.amazonaws.com",
            3306,
            "iam+user",
        )
        .expect("token");
        assert!(t.contains("DBUser=iam%2Buser"));
    }
}
