#!/usr/bin/env bash
# Mainnet-fork test runner. Starts surfnet (headless --ci) forking from MAINNET_RPC_URL,
# which auto-deploys target/deploy programs + airdrops our wallet, then runs ts-mocha against it.
# Usage: bash scripts/fork-test.sh [mocha globs...]   (default: tests/**/*.spec.ts)
set +e
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(dirname "$HERE")"
. "$HERE/wsl-env.sh"
cd "$REPO" || exit 1
sed -i 's/\r$//' .env 2>/dev/null
set -a; . ./.env; set +a
: "${MAINNET_RPC_URL:?set MAINNET_RPC_URL in .env}"
WALLET="$HOME/.config/solana/id.json"
RPC=http://127.0.0.1:8899

pkill -x surfpool 2>/dev/null; sleep 1
echo "starting surfnet (--ci headless, fork + airdrop; explicit deploy below) ...";
nohup env NO_DNA=1 surfpool start --ci --no-deploy -u "$MAINNET_RPC_URL" \
  --airdrop-keypair-path "$WALLET" --airdrop-amount 100000000000 >/tmp/surfpool.log 2>&1 &
SP_PID=$!
cleanup() { pkill -x surfpool 2>/dev/null; }
trap cleanup EXIT

probe() { curl -s "$RPC" -H 'Content-Type: application/json' -d "$1" 2>/dev/null; }
echo -n "waiting for RPC"; for i in $(seq 1 45); do
  probe '{"jsonrpc":"2.0","id":1,"method":"getSlot"}' | grep -q '"result"' && { echo " up"; break; }
  echo -n "."; sleep 1; done

# Explicitly deploy every built program that has a matching keypair (registry/dispatcher/adapters).
echo "=== deploying programs to surfnet ==="
for so in target/deploy/*.so; do
  [ -f "$so" ] || continue
  name="$(basename "$so" .so)"
  kp="keys/${name}-keypair.json"
  [ -f "$kp" ] || continue
  pid="$(solana-keygen pubkey "$kp")"
  echo "  deploy $name -> $pid"
  solana program deploy "$so" --program-id "$kp" --url "$RPC" --keypair "$WALLET" \
    --commitment confirmed --with-compute-unit-price 0 >/tmp/deploy_$name.log 2>&1 \
    || { echo "  DEPLOY FAILED ($name):"; tail -3 /tmp/deploy_$name.log; }
done

GLOBS="${*:-tests/**/*.spec.ts}"
echo "=== ts-mocha $GLOBS ==="
ANCHOR_PROVIDER_URL="$RPC" ANCHOR_WALLET="$WALLET" MAINNET_RPC_URL="$MAINNET_RPC_URL" \
  npx ts-mocha -p ./tsconfig.json -t 1000000 $GLOBS
RC=$?
echo "FORKTEST_EXIT=$RC"
exit $RC
