#!/usr/bin/env bash
# Post-push paranoia check: clone what's ACTUALLY on the remote into a throwaway dir and scan it
# for secrets — independent of your working tree. Also scans the published release artifacts.
#
# Usage:  ./scripts/verify-no-secrets.sh [git-remote-url]
# Requires: git, curl. Uses gitleaks (downloaded on demand). `gh` is optional (for release assets).
set -euo pipefail

REMOTE="${1:-$(git config --get remote.origin.url)}"
GITLEAKS_VER="8.21.2"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "▶ Verifying: $REMOTE"
echo "  scratch dir: $WORK"

# 1. Fresh clone (full history) — this is what the world would get, not your local tree.
git clone --quiet "$REMOTE" "$WORK/repo"

# 2. gitleaks over the entire history.
echo "▶ Scanning full git history for secrets…"
curl -sSLf "https://github.com/gitleaks/gitleaks/releases/download/v${GITLEAKS_VER}/gitleaks_${GITLEAKS_VER}_linux_x64.tar.gz" \
  | tar -xz -C "$WORK" gitleaks
if "$WORK/gitleaks" detect --source "$WORK/repo" --redact --no-banner --exit-code 1; then
  echo "  ✓ history clean"
else
  echo "  ✗ gitleaks flagged something above — investigate before trusting this push." >&2
  exit 1
fi

# 3. Published release artifacts (best-effort; needs `gh` authenticated).
if command -v gh >/dev/null 2>&1; then
  echo "▶ Scanning latest release artifacts…"
  mkdir -p "$WORK/assets"
  if gh release download --repo "$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || echo "")" \
       --dir "$WORK/assets" 2>/dev/null; then
    hits=0
    for f in "$WORK"/assets/*; do
      [ -e "$f" ] || continue
      if strings -n 8 "$f" | grep -aEq \
        'AKIA[0-9A-Z]{16}|ASIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY|xox[baprs]-|ghp_[A-Za-z0-9]{20}|sk-[A-Za-z0-9]{20}'; then
        echo "  ✗ potential secret in $(basename "$f")" >&2; hits=1
      fi
    done
    [ "$hits" -eq 0 ] && echo "  ✓ release artifacts clean"
    [ "$hits" -eq 0 ] || exit 1
  else
    echo "  (no release assets downloaded — skipping)"
  fi
else
  echo "▶ (gh not installed — skipping release-artifact scan)"
fi

echo "✓ All clear."
