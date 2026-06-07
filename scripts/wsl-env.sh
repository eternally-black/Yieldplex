#!/usr/bin/env bash
# Source in every WSL command:  . "<repo>/scripts/wsl-env.sh"
# Puts WSL-native bins FIRST so Windows PATH interop can't shadow anchor/solana.
export TMPDIR="$HOME/.tmp"
# NOTE: do NOT set CARGO_TARGET_DIR. We symlink <repo>/target -> a WSL-native dir instead,
# so cargo writes artifacts to fast ext4 while relative `target/deploy/*.so` paths still resolve
# (Anchor 1.0 LiteSVM tests include_bytes! the .so at ../../../target/deploy).
export PATH="$HOME/.cargo/bin:$HOME/.local/share/solana/install/active_release/bin:$HOME/.local/bin:$PATH"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
export NVM_DIR="$HOME/.nvm"
[ -s "$NVM_DIR/nvm.sh" ] && . "$NVM_DIR/nvm.sh" >/dev/null 2>&1
export YAS_DIR="/mnt/c/Users/Valera/Desktop/Earn Bounties/Solana Dex Adapters/yield-adapter-standard"
mkdir -p "$TMPDIR" "$CARGO_TARGET_DIR"
