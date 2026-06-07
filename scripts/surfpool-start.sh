#!/usr/bin/env bash
# Start surfnet (mainnet-fork) using the gitignored MAINNET_RPC_URL as datasource.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(dirname "$HERE")"
. "$HERE/wsl-env.sh"
set -a; . "$REPO/.env"; set +a
RPC="${MAINNET_RPC_URL:?MAINNET_RPC_URL not set in .env}"
cd "$REPO"
# NO_DNA=1 => non-interactive/headless (agent-friendly, no TUI).
exec env NO_DNA=1 surfpool start -u "$RPC" "$@"
