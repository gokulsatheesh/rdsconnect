# Contributing to rdsconnect

Thanks for wanting to help! This is a small, focused project — a native DB client
that makes IAM RDS auth invisible. Contributions that keep it small and sharp are very welcome.

## Dev setup

```bash
# 1. Toolchains
#    - Rust (stable): https://rustup.rs
#    - Node 20+
# 2. Linux system deps (Tauri / WebKitGTK)
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev build-essential libssl-dev libxdo-dev

# 3. Project
npm install
./scripts/fetch-rds-ca.sh     # RDS CA bundle (git-ignored)
npm run tauri dev
```

## Ground rules

- **Secrets stay in Rust.** The UI must never receive AWS credentials or raw auth tokens —
  only typed command results. If a change would leak a secret to the webview, it's wrong.
- **Nothing secret is persisted.** Auth always flows live through SSO on each (re)connect.
- **Match the surrounding style.** Rust: `cargo fmt` + `cargo clippy`. TS: existing patterns,
  `tsc --noEmit` clean.

## Before you open a PR

```bash
# frontend
npm run build            # tsc + vite build must pass
# rust
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test  --manifest-path src-tauri/Cargo.toml
```

## Commit messages

Short imperative subject, a blank line, then the *why*. Conventional-commits prefixes
(`feat:`, `fix:`, `chore:`) are welcome but not required.

## Good first issues

- Wire up the AWS CLI shell-out fallback in `token.rs` behind a feature flag.
- Add connection health/latency indicator to the sidebar.
- Result-set cell detail popover (for long / JSON values).
- Dark/light theme toggle.

Open an issue before large changes so we can agree on the approach.
