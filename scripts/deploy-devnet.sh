#!/usr/bin/env bash
# M8 — deploy registry + dispatcher + the 5 reference adapters to devnet, then initialize the registry
# and propose+approve all five. Protocols are absent on devnet, so execution (deposit/withdraw) is NOT
# run here — it is validated on mainnet-fork (tests/fork). This proves the governance/registry surface
# is live and the program ids match declare_id!. Idempotent: skips programs already deployed.
#
#   bash scripts/deploy-devnet.sh
#
# Needs ~16 SOL on the wallet (deployed with --max-len = exact .so size, no upgrade headroom).
set +e
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(dirname "$HERE")"
. "$HERE/wsl-env.sh"
cd "$REPO" || exit 1
sed -i 's/\r$//' .env 2>/dev/null
set -a; [ -f ./.env ] && . ./.env; set +a

WALLET="$HOME/.config/solana/id.json"
# Deploy via the Helius devnet RPC (derived from MAINNET_RPC_URL by mainnet->devnet). The public
# devnet RPC (api.devnet.solana.com) drops the many txs a program deploy needs. Falls back to the
# public RPC only if MAINNET_RPC_URL is unset.
if [ -n "$MAINNET_RPC_URL" ]; then
  URL="$(printf '%s' "$MAINNET_RPC_URL" | sed 's/mainnet/devnet/g')"
  echo "deploy RPC: Helius devnet (from MAINNET_RPC_URL)"
else
  URL="https://api.devnet.solana.com"
  echo "deploy RPC: public devnet (MAINNET_RPC_URL unset)"
fi
# registry + dispatcher + the 5 real adapters (mock/standin are test-only, not deployed to devnet).
# Override by passing an explicit program list (e.g. a subset that fits the current balance).
if [ "$#" -gt 0 ]; then
  PROGRAMS=("$@")
else
  PROGRAMS=(ya_registry ya_dispatcher ya_adapter_kamino ya_adapter_marginfi ya_adapter_jupiter_jlp ya_adapter_maple ya_adapter_drift_if)
fi

echo "wallet : $(solana address -k "$WALLET")"
echo "cluster: $(printf '%s' "$URL" | sed -E 's#(https?://[^/?]+).*#\1#')"
# Preflight: the deploy RPC must answer before we spend any SOL.
if ! solana cluster-version --url "$URL" >/dev/null 2>&1; then
  echo "ERROR: deploy RPC unreachable / key not valid for devnet ($URL). Aborting before spending SOL."
  exit 1
fi
echo "rpc    : reachable (cluster-version ok)"
BAL="$(solana balance -k "$WALLET" --url "$URL" | awk '{print $1}')"
echo "balance: ${BAL} SOL"
awk -v b="$BAL" 'BEGIN{ if (b+0 < 15) { print "WARNING: balance < 15 SOL — deploy may run out mid-way (each adapter ~2.6 SOL)."; } }'

for name in "${PROGRAMS[@]}"; do
  so="target/deploy/${name}.so"
  kp="keys/${name}-keypair.json"
  [ -f "$so" ] || { echo "MISSING $so — run scripts/build.sh first"; exit 1; }
  [ -f "$kp" ] || { echo "MISSING $kp"; exit 1; }
  pid="$(solana address -k "$kp")"
  if solana program show "$pid" --url "$URL" >/dev/null 2>&1; then
    echo "  skip $name ($pid) — already deployed"
    continue
  fi
  len="$(stat -c%s "$so")"
  echo "  deploy $name -> $pid (max-len $len) ..."
  solana program deploy "$so" \
    --program-id "$kp" \
    --keypair "$WALLET" \
    --url "$URL" \
    --max-len "$len" \
    --commitment confirmed \
    || { echo "  DEPLOY FAILED ($name). Re-run to resume (buffers are recoverable)."; exit 1; }
done

echo "=== initialize registry + propose/approve the 5 adapters ==="
ANCHOR_PROVIDER_URL="$URL" ANCHOR_WALLET="$WALLET" npx tsx "$HERE/setup-registry-devnet.ts" || exit 1

echo "=== verify ==="
ANCHOR_PROVIDER_URL="$URL" npx tsx "$HERE/verify-devnet.ts"
