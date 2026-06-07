#!/usr/bin/env bash
# Isolate which IDLs declare_program! can compile, one at a time.
set +e
HERE="$(cd "$(dirname "$0")" && pwd)"
. "$HERE/wsl-env.sh"
cd "$YAS_DIR" || exit 1
LIB="programs/ya-cpi-spike/src/lib.rs"
cp "$LIB" "$LIB.bak" 2>/dev/null

for name in kamino_lend marginfi jupiter_perps drift syrup_swap_pool; do
  cat > "$LIB" <<EOF
#![allow(unexpected_cfgs)]
#![allow(dead_code)]
extern crate anchor_lang;
use anchor_lang::declare_program;
declare_program!($name);
pub fn _id() -> anchor_lang::prelude::Pubkey { $name::ID }
EOF
  out="$(cargo check -p ya-cpi-spike 2>&1)"
  nerr="$(printf '%s\n' "$out" | grep -c '^error')"
  if [ "$nerr" -eq 0 ]; then
    echo "PASS  $name"
  else
    echo "FAIL  $name  ($nerr errors)"
    printf '%s\n' "$out" | grep '^error' | sort | uniq -c | sort -rn | head -6
  fi
done

# restore the 5-way lib
mv "$LIB.bak" "$LIB" 2>/dev/null
echo "done."
