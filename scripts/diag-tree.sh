#!/usr/bin/env bash
set +e
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(dirname "$HERE")"
. "$HERE/wsl-env.sh"
cd "$REPO" || exit 1
echo "=== duplicate solana-address / solana-pubkey in tree ==="
cargo tree -d 2>/dev/null | grep -iE "solana-address|solana-pubkey" -A3
echo "=== what pulls solana-address v2.x (ya-mock-adapter) ==="
cargo tree -p ya-mock-adapter -i "solana-address" 2>&1 | head -40
