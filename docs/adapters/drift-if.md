# Drift v2 Insurance Fund adapter (the two-phase adapter + the honest §F play)

USDC staking into the Drift Insurance Fund (spot market 0). This is the **two-phase** reference
adapter: `withdraw` opens a cooldown request, `settle_withdrawal` completes it after the unlock.

- Program: `ya_adapter_drift_if` `8MYJzh7Fm1q6QcrXNZNvCetoLkv1tfxjBDbrZXTFVjLs`
- Protocol: Drift v2 `dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH`

## Why this is an honest EDGE, not a live round-trip

**A live Drift IF-staking CPI is impossible for *any* submission today.** Drift's deployed program
has the insurance-fund-stake instructions **commented out of its `#[program]`** —
`drift-labs/protocol-v2` `programs/drift/src/lib.rs` ~lines **796–880**
(`initialize_/add_/request_remove_/cancel_request_remove_/remove_insurance_fund_stake`). The handler
bodies still exist in `controller/insurance.rs`; only the dispatch exports are disabled, so
re-enabling is a one-line-per-instruction uncomment. No validator or Surfpool setting changes this.

So we do **not** fake a live pass. We ship three things and prove each:

1. **A spec-correct adapter** (`programs/ya-adapter-drift-if/src/lib.rs`) with the real Drift account
   layout + discriminators (cross-checked against the IDL in `crates/ya-interface/src/cpi.rs`), the
   two-phase ticket, the cooldown read from chain, and the IF-share value math. It will execute the
   instant Drift re-enables the exports — it is **not** a stub.
2. **`yarn probe:drift-if`** — simulates each IF-staking discriminator against the live program and
   shows none execute. *Honest caveat:* a blind simulation rejects many instructions (our sim
   fee-payer isn't a live account), so the probe **corroborates** but the **authoritative** proof is
   the source comment-out above.
3. **A green two-phase lifecycle** (`tests/adapters/drift-if.spec.ts`) run against the labelled
   `ya-cooldown-standin` program — proving OUR machinery (the standard `request → cooldown → settle`
   withdrawal + the dispatcher's two-phase routing) is correct and reproducible:
   `deposit → withdraw(Pending, unlock_ts) → settle-too-early reverts (WithdrawalLocked) →
   surfnet time-travel +cooldown → settle (Settled)`. This is **never** presented as a live Drift pass
   (see `tests/fork/RESULTS.md`).

A competing submission marks Drift `describe.skip` and blames the validator version; the source proof
above shows that's incorrect — it's an upstream `#[program]` export, not tooling.

## How the (spec-correct) adapter works

| op | Drift CPI (`invoke_signed` by `vault_authority`) |
|---|---|
| `initialize_position` | Position + vault USDC + `initialize_user_stats` + `initialize_insurance_fund_stake(0)` (authority = `vault_authority`). |
| `deposit` | `transfer_checked` owner USDC → vault, then `add_insurance_fund_stake(0, amount)`. |
| `withdraw` | `request_remove_insurance_fund_stake(0, shares)`; ticket `Pending`, `unlock_ts = now + SpotMarket.insurance_fund.unstaking_period` (read from chain @ offset 384, currently 13 days). |
| `settle_withdrawal` | after unlock: `remove_insurance_fund_stake(0)` → vault → owner. |
| `current_value` | `if_shares × IF_vault_balance / total_if_shares` (oracle-free), **capped at `last_withdraw_request_value`** once a withdrawal is pending (§11 retroactive-price rule). |

Value math is a full 256-bit `mul_div_floor` (shares are `u128`) and is unit-tested
(`cargo test -p ya-adapter-drift-if`). The cooldown is read from chain, never hardcoded (C4).

## Run

- `yarn probe:drift-if` — live-program rejection + source citation.
- `bash scripts/fork-test.sh tests/adapters/drift-if.spec.ts` — the two-phase lifecycle on the stand-in.
- `bash scripts/test-rust.sh ya-adapter-drift-if` — IF-share value math.
