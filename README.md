<div align="center">

# rdsconnect

### Log into IAM-authenticated RDS MySQL without the token juggling.

A native Linux GUI database client that does AWS SSO → IAM auth → connect **invisibly**,
then lets you actually query. No 15-minute token copy-paste. No paywall.

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
(re)connect.

## Features

- **Zero token juggling** — sign in with AWS SSO once; the IAM auth token is generated and
  refreshed automatically on every connect. Existing sessions stay alive.
- **Guided connection setup** — pick account → role → region → RDS instance in a wizard, then
  save it for one-click reconnects.
- **TLS by default** — every connection is verified against the Amazon RDS CA; there is no
  plaintext path.
- **SSH tunnel support** — reach DBs that are only accessible from inside the VPC by connecting
  through a bastion / jump host. Uses your system `ssh` (agent, keys, `~/.ssh/config`); no SSH
  secrets are stored.
- **Multi-tab SQL editor** — CodeMirror with MySQL syntax highlighting; ⌘/Ctrl+↵ to run.
- **Fast results grid** — virtualized for large result sets, with one-click CSV export.
- **Schema browser** — databases → tables → columns, lazy-loaded on expand.
- **Saved connections & query history**, stored locally under `~/.config/rdsconnect/`.
- **Secrets stay in the core** — AWS credentials and auth tokens never reach the UI, and no
  secret is written to disk.

## Architecture

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

Download the latest build from the
[**Releases**](https://github.com/gokulsatheesh/rdsconnect/releases) page — no toolchain needed.
Two formats are published:

### AppImage — no install, no root

A single self-contained binary. Nothing to install, **no `sudo` required** — download, mark it
executable, and run:

```bash
wget https://github.com/gokulsatheesh/rdsconnect/releases/download/v1.0.0/rdsconnect_1.0.0_amd64.AppImage
chmod +x rdsconnect_1.0.0_amd64.AppImage
./rdsconnect_1.0.0_amd64.AppImage
```

### Debian / Ubuntu package (`.deb`)

Installs system-wide and adds rdsconnect to your application menu. Installing a system package
writes to `/usr/…`, so this route does need `sudo`:

```bash
wget https://github.com/gokulsatheesh/rdsconnect/releases/download/v1.0.0/rdsconnect_1.0.0_amd64.deb
sudo apt install ./rdsconnect_1.0.0_amd64.deb   # apt also pulls in the GTK/WebKit runtime
```

Then launch it from your application menu or run `rdsconnect`.

On first run, open **Settings** and enter your organization's **AWS SSO start URL** and region.
The DB user defaults to your OS username and regions default to `us-east-1` — all editable.

> **Fleet / MDM rollout (JumpCloud, etc.):** the `.deb` is a standard package, so a one-line
> command policy — `apt install -y ./rdsconnect_1.0.0_amd64.deb` after fetching it — installs it
> across managed laptops.

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

## Requirements

- **Linux** with a WebKitGTK runtime (installed automatically by the `.deb`).
- An **AWS SSO** (IAM Identity Center) start URL for your organization.
- The target RDS MySQL user must exist with IAM authentication enabled server-side
  (`AWSAuthenticationPlugin` / the `rds_iam` role). rdsconnect authenticates that user; it does
  not provision it.

Contributions are welcome — see [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

[MIT](LICENSE) © 2026 Gokul Satheesh
