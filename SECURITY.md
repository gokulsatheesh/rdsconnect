# Security Policy

## Design principles

- **Secrets stay in the Rust core.** AWS credentials and RDS auth tokens never cross into the
  webview. The UI calls typed commands and only receives non-secret results.
- **Nothing sensitive is persisted long-term.** RDS auth tokens are generated on demand and used
  immediately; they are never written to disk. The only cached secret is the SSO *access token*
  (so you aren't prompted to log in every launch), stored at `~/.config/rdsconnect/sso.json`
  with `0600` permissions.
- **TLS is mandatory.** IAM RDS auth requires TLS, so there is no plaintext fallback. **Direct**
  connections use `VerifyIdentity` — the server cert is verified against the bundled Amazon RDS CA
  for both chain **and** hostname. **SSH-tunneled** connections use `Required`: TLS is still
  enforced (so the IAM auth token is never sent in the clear), but the cert is not verified,
  because the socket target is `127.0.0.1` and sqlx 0.8 + rustls 0.23 cannot currently skip *only*
  the hostname check (`VerifyCa` errors on the name mismatch). For tunnels, host authentication is
  instead provided by the SSH hop to a trusted bastion. (If sqlx regains working hostname-skip,
  tunnels should move back to CA verification.)
- **SSH tunnels carry no in-app secrets.** Optional bastion/jump-host tunneling shells out to the
  system `ssh` client, delegating all authentication to the user's existing SSH agent, keys,
  `~/.ssh/config`, and `known_hosts`. rdsconnect never handles or stores SSH keys or passphrases.
- **Least privilege in the webview.** The Tauri capability grants `core:default` only — no
  filesystem, shell, or HTTP permissions are exposed to the frontend. A restrictive CSP is set.
- **Parameterized queries.** All app-generated queries (schema/table introspection) use bound
  parameters, never string interpolation. The SQL editor runs exactly what the user types — by
  design, since this is a database client.

## Secret & dependency scanning

Every push and PR runs [`.github/workflows/security.yml`](.github/workflows/security.yml):
**gitleaks** over the full git history (secrets) and **`cargo audit`** + **`npm audit`**
(dependency SAST). The release workflow reuses this job as a **gate** — no tagged release is
built unless it passes — and additionally scans the built `.deb`/`.AppImage` for secret patterns
before uploading. For a post-push spot check you can run
[`scripts/verify-no-secrets.sh`](scripts/verify-no-secrets.sh), which clones the *remote* into a
throwaway dir and scans it (plus the published release assets) independently of your working tree.

## Known transitive advisories

These come from dependencies we don't control directly and can't fix without upstream releases;
Dependabot (`.github/dependabot.yml`) opens PRs as fixes land. `cargo audit` is configured to fail
on any *new* advisory while ignoring exactly this documented set (all low-exploitability for how
this app uses the dependency):

- **`rsa`** (`RUSTSEC-2023-0071`, Marvin timing side-channel) — no fixed release exists; pulled in
  by `sqlx-mysql` for MySQL auth, which in this app always runs inside TLS.
- **`rustls-webpki`** (`RUSTSEC-2026-0098`, `-0099` name-constraint acceptance; `-0104` CRL parse
  panic) — via the **AWS SDK's** bundled `rustls 0.21`, used only for TLS to fixed AWS endpoints;
  RDS certs don't use name constraints and no attacker-supplied CRLs are parsed. The app's
  *database* TLS uses a separate, current `rustls-webpki`.
- **`quick-xml`** (`RUSTSEC-2026-0194`, `-0195`, parsing DoS) — via the **AWS SDK's** XML
  deserializer, which only parses responses from AWS's own APIs.

Additionally, `cargo audit` reports (but does **not** fail on) *unmaintained* advisories for
Tauri's GTK3 bindings (`atk`/`gdk`/`gtk`/`glib`/…) and a few transitive parse crates
(`unic-*`, `proc-macro-error`). These are inherent to Tauri v2 on Linux and carry no fix.

## Supported versions

This project is pre-1.0; security fixes land on `main`.

## Reporting a vulnerability

Please report suspected vulnerabilities privately via GitHub Security Advisories
("Report a vulnerability" on the repo's **Security** tab) rather than a public issue.
We'll acknowledge within a few days and keep you updated on the fix.
