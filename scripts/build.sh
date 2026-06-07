#!/usr/bin/env bash
# Anchor SBF build with the edition2024 Cargo.lock pins (platform-tools cargo 1.84 rejects
# edition=2024) and our fixed program keypairs placed where anchor expects them.
set +e
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(dirname "$HERE")"
. "$HERE/wsl-env.sh"
cd "$REPO" || exit 1

# Use our stable program keypairs (declare_id! pubkeys) for the build/deploy.
mkdir -p target/deploy
rm -f target/deploy/*-keypair.json
cp keys/*-keypair.json target/deploy/ 2>/dev/null

# Resolve, then pin the edition2024-breaking crates (no-op if absent).
cargo generate-lockfile 2>/dev/null
cargo update -p blake3 --precise 1.8.2 2>/dev/null
cargo update -p constant_time_eq --precise 0.3.1 2>/dev/null
cargo update -p base64ct --precise 1.7.3 2>/dev/null
cargo update -p indexmap --precise 2.11.4 2>/dev/null

echo "=== anchor build $* ==="
NO_DNA=1 anchor build "$@" 2>&1 | tail -45
echo "BUILD_EXIT=${PIPESTATUS[0]}"
ls -la target/deploy/*.so 2>/dev/null
