# Security Policy

## Design principles

- **Secrets stay in the Rust core.** AWS credentials and RDS auth tokens never cross into the
  webview. The UI calls typed commands and only receives non-secret results.
- **Nothing sensitive is persisted long-term.** RDS auth tokens are generated on demand and used
  immediately; they are never written to disk. The only cached secret is the SSO *access token*
  (so you aren't prompted to log in every launch), stored at `~/.config/rdsconnect/sso.json`
  with `0600` permissions.
- **TLS is mandatory.** IAM RDS auth requires TLS; every connection uses `VerifyIdentity` SSL mode
  against the bundled Amazon RDS CA. There is no plaintext fallback.
- **Least privilege in the webview.** The Tauri capability grants `core:default` only — no
  filesystem, shell, or HTTP permissions are exposed to the frontend. A restrictive CSP is set.
- **Parameterized queries.** All app-generated queries (schema/table introspection) use bound
  parameters, never string interpolation. The SQL editor runs exactly what the user types — by
  design, since this is a database client.

## Known transitive advisories

These come from dependencies we don't control directly and can't fix without upstream
releases; Dependabot (`.github/dependabot.yml`) opens PRs as fixes land. Both are low
exploitability for this app's usage:

- **`rustls-webpki` 0.101.7** (RUSTSEC / GHSA-82j2-j2ch-gfr8, DoS panic on a malformed CRL) —
  pulled in by the **AWS SDK's** own TLS client (`aws-smithy-http-client` → `rustls 0.21`), used
  only for calls to AWS endpoints (fixed CA, no attacker-supplied CRLs). The app's *database*
  TLS uses a separate, current rustls-webpki.
- **`glib` 0.18.5** (GHSA-wrw7-89jp-8q8g, iterator unsoundness) — pulled in by **Tauri's GTK
  bindings**; the affected `VariantStrIter` path is not used by this app.

## Supported versions

This project is pre-1.0; security fixes land on `main`.

## Reporting a vulnerability

Please report suspected vulnerabilities privately via GitHub Security Advisories
("Report a vulnerability" on the repo's **Security** tab) rather than a public issue.
We'll acknowledge within a few days and keep you updated on the fix.
