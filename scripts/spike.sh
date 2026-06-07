#!/usr/bin/env bash
# M0 declare_program! spike: compile-check declare_program! against all 5 vendored IDLs.
set +e
HERE="$(cd "$(dirname "$0")" && pwd)"
. "$HERE/wsl-env.sh"
cd "$YAS_DIR" || exit 1

# Fast WSL-native artifacts via a target symlink (keeps relative target/ paths working).
TGT="$HOME/.cache/yas-target"
mkdir -p "$TGT"
if [ ! -e target ]; then ln -s "$TGT" target; fi
echo "target -> $(readlink target 2>/dev/null || echo '(real dir)')"
echo "idls:"; ls -1 idls/

echo "=== cargo check -p ya-cpi-spike (declare_program! x5) ==="
cargo check -p ya-cpi-spike 2>&1 | tail -50
echo "CHECK_EXIT=${PIPESTATUS[0]}"
