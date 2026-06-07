#!/usr/bin/env bash
# Generate stable program keypairs (gitignored). Pubkeys go into declare_id! + Anchor.toml.
set +e
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(dirname "$HERE")"
. "$HERE/wsl-env.sh"
cd "$REPO" || exit 1
mkdir -p keys
for name in ya_registry ya_dispatcher ya_mock_adapter ya_adapter_kamino ya_adapter_marginfi ya_adapter_drift_if ya_adapter_jupiter_jlp ya_adapter_maple; do
  f="keys/${name}-keypair.json"
  [ -f "$f" ] || solana-keygen new -o "$f" --no-bip39-passphrase --silent --force >/dev/null 2>&1
  printf "%-26s %s\n" "$name" "$(solana-keygen pubkey "$f")"
done
