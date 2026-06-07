#!/usr/bin/env bash
# Fetch protocol IDLs on-chain via `anchor idl fetch` into idls/.
# Programs without an on-chain IDL account are reported for manual GitHub vendoring.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
. "$HERE/wsl-env.sh"
set -a; . "$YAS_DIR/.env"; set +a
RPC="${MAINNET_RPC_URL:?MAINNET_RPC_URL not set}"
cd "$YAS_DIR"
mkdir -p idls

names=(kamino_lend marginfi jupiter_perps drift)
ids=(KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD \
     MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA \
     PERPHjGBqRHArX4DySjwM6UJHiR3sWAatqfdBS2qQJu \
     dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH)

for i in "${!names[@]}"; do
  name="${names[$i]}"; pid="${ids[$i]}"
  echo "=== $name ($pid) ==="
  if anchor idl fetch -o "idls/$name.json" "$pid" --provider.cluster "$RPC" 2>/tmp/idl_err.txt; then
    echo "  OK: $(wc -c < "idls/$name.json") bytes -> idls/$name.json"
  else
    echo "  FAILED (no on-chain IDL or error): $(tail -2 /tmp/idl_err.txt | tr '\n' ' ')"
  fi
done
echo "done."
