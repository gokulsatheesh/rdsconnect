<div align="center">

# rdsconnect

### Log into IAM-authenticated RDS MySQL without the token juggling.

A native Linux GUI database client that does AWS SSO → IAM auth → connect **invisibly**,
then lets you actually query. No 15-minute token copy-paste. No paywall.

*(The GUI successor to a `rds-connect.sh` shell script.)*

[![ci](https://github.com/gokulsatheesh/rdsconnect/actions/workflows/ci.yml/badge.svg)](https://github.com/gokulsatheesh/rdsconnect/actions/workflows/ci.yml)
[![release](https://github.com/gokulsatheesh/rdsconnect/actions/workflows/release.yml/badge.svg)](https://github.com/gokulsatheesh/rdsconnect/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-informational.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-Linux-333)
![Built with](https://img.shields.io/badge/built%20with-Tauri%20%C2%B7%20Rust%20%C2%B7%20React-5aa2ff)

</div>

---

## Why

AWS RDS supports **IAM database authentication**: instead of a static password, you generate a
short-lived (15-minute) auth token and use it as the DB password. It's great for security but
tedious day to day — most GUI clients don't support it, or lock SSO + IAM auth behind a paywall.

`rdsconnect` makes it a first-class, invisible flow: **AWS SSO login → account → role → region →
instance → auto-generated token → connect**, with the token transparently regenerated on every
(re)connect. On top of that it's a real client — schema browser, multi-tab SQL editor,
virtualized results grid, saved connections, and query history.

## How it works

- **Rust core** (`src-tauri/`) owns all AWS + DB I/O and every secret. The UI never sees AWS
  creds or raw tokens — it calls typed Tauri commands.
- The auth token is a **SigV4-presigned string** (`rds-db` service, 900 s) used as the DB
  password, regenerated transparently on every (re)connect. **Nothing secret is persisted.**
- IAM RDS auth **requires TLS** with the Amazon RDS CA (`global-bundle.pem`, bundled at build).

| Module | Job |
| --- | --- |
| [`sso.rs`](src-tauri/src/sso.rs) | Device-auth login + `~/.aws/sso` cache reuse |
| [`aws.rs`](src-tauri/src/aws.rs) | list-accounts / list-roles / get-role-credentials / describe-db-instances |
| [`token.rs`](src-tauri/src/token.rs) | SigV4 presign → RDS auth token (`generate-db-auth-token` equivalent) |
| [`db.rs`](src-tauri/src/db.rs) | MySQL connect (TLS + token), query, schema introspection, transparent reconnect |
| [`connections.rs`](src-tauri/src/connections.rs) | Saved connections, history, settings under `~/.config/rdsconnect/` |
| [`commands.rs`](src-tauri/src/commands.rs) | Thin `#[tauri::command]` layer |
| [`state.rs`](src-tauri/src/state.rs) | Cached SSO token, per-role cred cache, active DB pools |

## Install

Grab the latest build from the [**Releases**](https://github.com/gokulsatheesh/rdsconnect/releases)
page — no toolchain needed.

**Debian / Ubuntu (`.deb`)**
```bash
sudo apt install ./rdsconnect_*_amd64.deb     # pulls in the GTK/WebKit runtime automatically
```

**Anything else (`.AppImage`)** — a single self-contained binary:
```bash
chmod +x rdsconnect_*.AppImage
./rdsconnect_*.AppImage
```

On first run, open **Settings** and enter your organization's **AWS SSO start URL** and region.
The DB user defaults to your OS username and regions default to `us-east-1` — all editable.

> **Fleet / MDM rollout (JumpCloud, etc.):** the `.deb` is a standard package, so a one-line
> command policy installs it on every managed laptop:
> `apt install -y ./rdsconnect_<version>_amd64.deb` (after fetching it from the release).

## Build from source

```bash
# Toolchains: Rust (https://rustup.rs) + Node 20+
# Linux system deps (Tauri / WebKitGTK):
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev build-essential libssl-dev libxdo-dev

npm install
./scripts/fetch-rds-ca.sh        # download the RDS CA bundle (git-ignored)
npm run tauri dev                # run in dev mode
npm run tauri build              # → .deb + .AppImage under src-tauri/target/release/bundle/
```

Tagging `vX.Y.Z` triggers the release workflow, which builds and uploads both artifacts to a
GitHub release.

## Status

> Early but building. **Verified:** the Rust core compiles clean and passes
> `cargo fmt --check`, `cargo clippy -D warnings`, and `cargo test` (SigV4 token presign);
> the frontend passes `npm run build`. Next up is exercising the live auth + DB flow
> end-to-end against a real RDS instance.

**Roadmap** (see [`CONTRIBUTING.md`](CONTRIBUTING.md) for good first issues)

1. ✅ Scaffold — app + `ping` round-trip
2. Auth core (`sso`/`aws`/`token`) — reproduce the script end-to-end
3. DB connect + query over TLS with the token
4. Connection wizard + saved connections + sidebar
5. Schema tree + multi-tab editor + results grid + CSV
6. History + settings polish
7. Packaging: `.deb` + `.AppImage`

## Notes / risks

- MySQL long-token auth may need `sqlx`'s cleartext-password path; if `sqlx` resists,
  `mysql_async` is the fallback driver.
- If `aws-sigv4` presigning fights us, `token.rs` can shell out to `aws rds
  generate-db-auth-token` as a stopgap, then swap back to native.
- Server-side IAM DB user setup (`AWSAuthenticationPlugin`, `rds_iam`) is assumed done.

## License

[MIT](LICENSE) © 2026 Gokul Satheesh
