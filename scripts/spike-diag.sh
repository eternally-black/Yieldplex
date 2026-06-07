#!/usr/bin/env bash
# Show full compiler context for a single IDL's declare_program! errors.
set +e
HERE="$(cd "$(dirname "$0")" && pwd)"
. "$HERE/wsl-env.sh"
cd "$YAS_DIR" || exit 1
NAME="${1:-kamino_lend}"
cat > programs/ya-cpi-spike/src/lib.rs <<EOF
#![allow(unexpected_cfgs)]
#![allow(dead_code)]
use anchor_lang::declare_program;
declare_program!($NAME);
EOF
echo "=== full errors for declare_program!($NAME) ==="
cargo check -p ya-cpi-spike 2>&1 | grep -A 20 "error\[" | head -80
