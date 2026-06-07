#!/usr/bin/env bash
# Verify the 3-way declare_program! spike + dump CPI metadata. Repo root derived from $0.
set +e
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(dirname "$HERE")"
. "$HERE/wsl-env.sh"   # tools/PATH only; cd uses $REPO (robust to any YAS_DIR CRLF)
cd "$REPO" || { echo "cd REPO failed: [$REPO]"; exit 1; }
echo "cwd: $(pwd)"
echo "=== 3-way declare_program! spike (kamino_lend + jupiter_perps + orca whirlpool) ==="
cargo check -p ya-cpi-spike 2>&1 | tail -6
echo "CHECK_EXIT=${PIPESTATUS[0]}"
echo ""
echo "=== CPI metadata dump (manual path for marginfi/drift + SDK builders) ==="
node scripts/dump-cpi-meta.mjs 2>&1 | head -150
