# Fork-only fixtures

Every patch the fork tests apply to cloned mainnet state. **No production code path depends on any of
these** — they only make a deterministic test possible on a fork (cloned oracles go stale relative to
the fork clock; lazily-accruing positions need a fixed instant; cooldowns need time-travel). All
on-chain reads/writes here are local to Surfpool; we never send a mainnet transaction.

## 1. Jupiter JLP — refresh the pool's price oracles (`tests/adapters/jlp.spec.ts`)
- **Why:** `add/remove_liquidity2` revalue the whole pool and reject price accounts older than
  `custody.oracle.max_price_age_sec = 5s`. A cloned fork's oracle `publish_time` is older than the
  advancing fork clock, so the op would revert.
- **Patch:** before each deposit/withdraw, `surfnet_setAccount` rewrites the `i64` `publish_time`
  (offset 177) of the 5 pool `doves_ag` oracles to the current fork clock. Addresses unchanged; only
  the timestamp byte is bumped. Production fails closed on a genuinely stale oracle.

## 2. MarginFi — pin the clock for the same-instant diff=0 (`tests/adapters/marginfi.spec.ts`)
- **Why:** MarginFi accrues interest lazily (per second, only on deposit/withdraw). `current_value`
  read earlier than the redemption understates by the interest accrued in between (conservative,
  never overstates). To assert a *same-instant* `diff = 0`, both observations must be in the same unix
  second.
- **Patch (EDGE test only):** `surfnet_timeTravel` pins the deposit and the withdraw to two
  `absoluteTimestamp` values within the same second (ms-precision, strictly increasing so neither is
  "in the past"). Without the pin the diff is ≤ 1 lamport of genuine accrued interest.

## 3. Drift IF stand-in — cross the cooldown (`tests/adapters/drift-if.spec.ts`)
- **Why:** the two-phase lifecycle requires advancing past the 13-day cooldown unlock.
- **Patch:** `surfnet_timeTravel` advances the fork clock by `cooldown + 60s` between the `request`
  (Pending) and the `settle`. Run in its own fork so the +13-day jump never affects other adapters'
  oracle freshness.

## Cheatcode param shapes (surfnet, verified live)
- `surfnet_setTokenAccount`: `amount` is a JSON **u64 number** (not a string).
- `surfnet_setAccount`: `data` is a **hex** string; pass `{ lamports, data, owner, executable }`.
- `surfnet_timeTravel`: `{ absoluteTimestamp }` in **milliseconds**; cannot travel backward.
