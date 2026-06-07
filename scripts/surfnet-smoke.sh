#!/usr/bin/env bash
# Smoke-test surfnet: start it forking from MAINNET_RPC_URL, confirm validator up + fork works
# + whether the workspace programs are auto-deployed. Leaves surfnet running in the background.
set +e
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(dirname "$HERE")"
. "$HERE/wsl-env.sh"
cd "$REPO" || exit 1
sed -i 's/\r$//' .env
set -a; . ./.env; set +a
echo "URL length: ${#MAINNET_RPC_URL}   tail: ...$(printf '%s' "$MAINNET_RPC_URL" | tail -c 10)"

pkill -f 'surfpool start' 2>/dev/null; sleep 1
echo "starting surfnet (NO_DNA=1, port 8899) ..."
nohup env NO_DNA=1 surfpool start -u "$MAINNET_RPC_URL" --port 8899 >/tmp/surfpool.log 2>&1 &
echo "pid=$!"
sleep 30

RPC=http://127.0.0.1:8899
q() { curl -s "$RPC" -H 'Content-Type: application/json' -d "$1"; echo; }

echo "=== surfpool.log tail ==="; tail -15 /tmp/surfpool.log
echo "=== getSlot ==="; q '{"jsonrpc":"2.0","id":1,"method":"getSlot"}'
echo "=== USDC mint present (fork works)? ==="; q '{"jsonrpc":"2.0","id":1,"method":"getAccountInfo","params":["EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",{"encoding":"base64","dataSlice":{"offset":0,"length":0}}]}'
echo "=== ya_registry auto-deployed? ==="; q '{"jsonrpc":"2.0","id":1,"method":"getAccountInfo","params":["3ehQoDePP3eULnSKxgHc6DvLAwEQNeVHvJYzWPXoQyUD",{"encoding":"base64","dataSlice":{"offset":0,"length":0}}]}'
echo "=== ya_dispatcher auto-deployed? ==="; q '{"jsonrpc":"2.0","id":1,"method":"getAccountInfo","params":["2aY1hBVBJJmX8uSgB4aqhuS2xeDaGCc3d55KE2Mbvvgs",{"encoding":"base64","dataSlice":{"offset":0,"length":0}}]}'
