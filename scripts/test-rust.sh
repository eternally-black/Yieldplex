#!/usr/bin/env bash
# Run a crate's Rust (LiteSVM) tests. Usage: test-rust.sh <crate> [test filter]
set +e
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(dirname "$HERE")"
. "$HERE/wsl-env.sh"
cd "$REPO" || exit 1
CRATE="${1:-ya-registry}"
shift 2>/dev/null
echo "=== cargo test -p $CRATE ==="
cargo test -p "$CRATE" "$@" -- --nocapture --test-threads=1 2>&1 | tail -50
echo "TEST_EXIT=${PIPESTATUS[0]}"
