#!/usr/bin/env bash
# Download the Amazon RDS global CA bundle that IAM auth (TLS) requires.
# Run once after cloning; the bundle is git-ignored and bundled into the app at build time.
set -euo pipefail

DEST="$(cd "$(dirname "$0")/.." && pwd)/src-tauri/resources/global-bundle.pem"
URL="https://truststore.pki.rds.amazonaws.com/global/global-bundle.pem"

echo "⤓ Fetching RDS CA bundle → $DEST"
curl -fsSL "$URL" -o "$DEST"
echo "✓ $(grep -c 'BEGIN CERTIFICATE' "$DEST") certificates written."
