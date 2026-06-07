#!/usr/bin/env bash
# Make target/deploy/*-keypair.json our fixed keys/ keypairs, so anchor's program ids match declare_id!.
set +e
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(dirname "$HERE")"
. "$HERE/wsl-env.sh"
cd "$REPO" || exit 1
mkdir -p target/deploy
rm -f target/deploy/*-keypair.json
cp keys/*-keypair.json target/deploy/
echo "keys/ya_registry:    $(solana-keygen pubkey keys/ya_registry-keypair.json)"
echo "deploy/ya_registry:  $(solana-keygen pubkey target/deploy/ya_registry-keypair.json)"
echo "--- anchor keys list ---"
NO_DNA=1 anchor keys list 2>&1 | head -20
