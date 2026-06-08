# Security checklist — ya-adapter-drift-if

**Risk level: 🔴 Critical** — two-phase (cooldown) IF staking via the per-position `vault_authority`
PDA. The 5th reference adapter and the two-phase reference (request → cooldown → settle).

> Note: a live Drift IF-staking CPI is disabled upstream (commented out of Drift's `#[program]`), so
> this adapter is **spec-correct but not live-runnable today** — see `docs/adapters/drift-if.md` +
> `yarn probe:drift-if`. Its two-phase machinery is proven green on `ya-cooldown-standin`.

Derived from `safe-solana-builder` (Frank Castle). Applied:

## CPI safety (shared-base §5)
- [x] **One external CPI per op** into Drift (`dRifty…`, hardcoded `DRIFT_ID`, asserted on the program account passed in remaining + on every Drift account's owner). 5 Drift instructions: `initialize_user_stats`, `initialize_insurance_fund_stake` (init), `add_insurance_fund_stake` (deposit), `request_remove_insurance_fund_stake` (withdraw), `remove_insurance_fund_stake` (settle). Built via the uniform `ya_interface::CpiCall`; discriminators cross-checked against the IDL in `cpi.rs`.
- [x] **`invoke_signed` by `vault_authority`** (the IF-stake authority, set at init); canonical bump stored on `Position` and reused.
- [x] CPI errors propagated with `?`.

## Two-phase withdrawal (SPEC §4.4, §C3/C4)
- [x] `withdraw` = `request_remove_insurance_fund_stake`; writes a `Pending` `WithdrawalTicket` with `unlock_ts = now + unstaking_period`.
- [x] **Cooldown read from chain**, never hardcoded: `SpotMarket.insurance_fund.unstaking_period` (i64 @ offset 384), validated owner + discriminator first.
- [x] `settle_withdrawal` requires `now >= ticket.unlock_ts` (`WithdrawalLocked` otherwise) and a `Pending` ticket (`NothingToSettle` otherwise), then `remove_insurance_fund_stake` → vault → owner via `transfer_checked`. Balance-delta payout (§21.6).
- [x] Lifecycle proven on the stand-in: `deposit → request(Pending) → too-early settle reverts → time-travel +cooldown → settle(Settled)`.

## remaining_accounts validation (shared-base §1.4 / §9.3 / W011)
- [x] Every Drift account read (SpotMarket, InsuranceFundStake) is owner-checked (`== DRIFT_ID`) + discriminator-checked + length-checked before any offset read.
- [x] The Drift program account passed in remaining is asserted `== DRIFT_ID`.

## Value math (`current_value`, §3 / §11)
- [x] **Oracle-free:** `if_shares × IF_vault_balance / total_if_shares` — pure IF-share accounting, no price feed.
- [x] **§11 retroactive-price cap:** once a withdrawal is pending (`last_withdraw_request_shares > 0`), `current_value` is capped at `last_withdraw_request_value` — the user can never report (or realize) more than the value snapshotted at request time.
- [x] Full 256-bit `mul_div_floor` (IF shares are `u128`), checked throughout; unit-tested.

## Account validation / lifecycle
- [x] Typed prefix (`Position`, `has_one`, canonical bumps); `vault_authority` seed-constrained; adapter asserts the registry entry (`load_adapter_entry`).
- [x] `initialize_position` idempotent (guarded by `owner == default`; vault USDC `init_if_needed`; the Drift init CPIs run once).

## High-risk decisions / known limitations
- **Live CPI disabled upstream** — the headline limitation, stated plainly (docs + RESULTS). The adapter ships spec-correct and runs the instant Drift re-enables the exports.
- IF-share value uses the floor branch of Drift's `get_proportion_u128`; the rare majority-staker ceiling branch is documented as a follow-up (cannot be fork-verified while live CPI is disabled).
- Upgradeable program; upgrade authority should be the governance multisig in production.

## Tests
- [x] Rust unit (`cargo test -p ya-adapter-drift-if`): IF-share value math (incl. large-u128 shares).
- [x] `yarn probe:drift-if`: live program rejects all IF-staking discriminators (corroborating; source is authoritative).
- [x] Surfpool (`tests/adapters/drift-if.spec.ts`): the full two-phase lifecycle on `ya-cooldown-standin` — NOT a live Drift pass.
