#!/usr/bin/env bash
# M1 validation: ya-interface compiles + discriminator test passes + macro expands in a real program.
set +e
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(dirname "$HERE")"
. "$HERE/wsl-env.sh"
cd "$REPO" || exit 1
echo "=== cargo check -p ya-interface ==="
cargo check -p ya-interface 2>&1 | tail -25
echo "IFACE_EXIT=${PIPESTATUS[0]}"
echo ""
echo "=== cargo test -p ya-interface (discriminator cross-check vs IDL bytes) ==="
cargo test -p ya-interface 2>&1 | tail -16
echo "TEST_EXIT=${PIPESTATUS[0]}"
echo ""
echo "=== cargo check -p ya-test-adapter (macro expansion + ergonomics under #[program]) ==="
cargo check -p ya-test-adapter 2>&1 | tail -35
echo "ADAPTER_EXIT=${PIPESTATUS[0]}"
