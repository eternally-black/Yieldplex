# Security checklist — ya-adapter-marginfi

**Risk level: 🔴 Critical** — the position is a `marginfi_account` whose authority is the per-position
`vault_authority` PDA; the adapter CPIs USDC in/out of MarginFi v2 and computes redeemable value from
I80F48 share math.

Derived from `safe-solana-builder` (Frank Castle). Applied:

## CPI safety (shared-base §5)
- [x] **One external CPI per op** into MarginFi v2 (`MFv2hW…`, hardcoded `MARGINFI_ID`, asserted on the program account passed in `remaining_accounts`). init → `marginfi_account_initialize`; deposit → `lending_account_deposit`; withdraw → `lending_account_withdraw`. Built via the uniform `ya_interface::CpiCall`.
- [x] **`invoke_signed` by `vault_authority`** for deposit/withdraw (the stored `marginfi_account.authority`). `initialize_position` signs for **both** the `marginfi_account` PDA and `vault_authority` (two seed sets) so the marginfi account is a program-owned PDA, not a loose keypair.
- [x] **Self-contained, no crank, no introspection:** deposit/withdraw accrue interest internally in-slot; no separate top-level refresh; neither standalone instruction has a CPI guard (source-verified). Call tree stays at depth ≤ 4.
- [x] CPI errors propagated with `?`. Deposit takes `(amount, deposit_up_to_limit: Option<bool>=None)` — the deployed program's arity (verified empirically on the fork).
- [x] Withdraw appends the post-withdraw **health check** accounts `[bank, oracle]` as the CPI's trailing remaining_accounts (the marginfi RiskEngine requires `[bank, oracle]` per active balance; `withdraw_all` closes the balance first, so they're then ignored — safe to always pass).

## remaining_accounts validation (shared-base §1.4 / §9.3 / W011)
- [x] **Bank:** owner == `MARGINFI_ID`, 8-byte discriminator, length checked before any offset read.
- [x] Bound to the position: `bank.mint == base_mint`, `bank.group == the group passed in`, and the marginfi program account == `MARGINFI_ID`. marginfi re-derives + checks its own `liquidity_vault` / `liquidity_vault_authority` PDAs internally.
- [x] **Vault accounts:** the prefix `vault_token_account` (vault USDC) and the `marginfi_account` are re-derived as this position's canonical PDAs (`[b"vault_usdc"|b"marginfi_account", position]`); the marginfi_account is asserted marginfi-owned.

## Token handling (shared-base §7 / §21.6)
- [x] **Balance-delta accounting:** withdraw measures the *actual* USDC delta on the vault account (re-read after the CPI), not the requested amount; deposit credits `shares` by the deposited principal (marginfi pulls exactly `amount`).
- [x] SPL `transfer_checked` (decimals from the mint) — owner→vault on deposit (owner signs), vault→owner on withdraw (`vault_authority` signs). Per-user vault PDA, no global vault (§5.8). Every position has a withdrawal path (§22.1).
- [x] Slippage: `min_position_out` (deposit, deterministic = amount) / `min_amount_out` (withdraw) → `SlippageExceeded`.

## Value math (`current_value`, shared-base §3 / §11 / §21)
- [x] value = **our** `asset_shares` for the bank (read from our marginfi_account's lending_account) × `Bank.asset_share_value` — i.e. the **position's** redeemable amount, never a pool aggregate.
- [x] **Byte-exact with the chain:** `floor(asset_shares × asset_share_value)` = `(shares_bits × asv_bits) >> 96`, both `WrappedI80F48` (×2⁴⁸), computed via a hand-rolled full 256-bit `mul_u128` (the product overflows u128). Matches marginfi's `get_asset_amount(shares).checked_floor()` (truncating I80F48 mul). Pinned by a unit test to a live bank snapshot; the fork EDGE test asserts `current_value == actual redeemed USDC, diff = 0`.
- [x] Checked arithmetic; INITIAL 1:1 implicitly handled (asv = 2⁴⁸ → value = shares).
- [x] **Conservative:** `current_value` reads the last in-slot-accrued `asset_share_value`, so it never overstates the redeemable amount (interest is only ever booked higher on withdraw). The EDGE test pins the surfnet clock so the value-read and the redemption accrue to the same instant, proving exact equality (fork-only fixture, `tests/fork/FIXTURES.md`).

## Account validation / lifecycle
- [x] Typed prefix (`Position`, `has_one = owner/base_mint`, canonical bumps); `vault_authority` seed-constrained.
- [x] Adapter asserts the registry entry itself (`load_adapter_entry` → owner+disc+canonical-PDA+program_id, then `status == Active` + `base_mint`).
- [x] `initialize_position` is idempotent: Position guarded by `owner == default`; vault USDC `init_if_needed`; the marginfi_account CPI is gated on `data_is_empty()`.

## High-risk decisions / known limitations
- The marginfi_account is a program-owned PDA `[b"marginfi_account", position]` (deterministic, no stored keypair). authority = `vault_authority`, so a single PDA controls init + every op.
- USDC bank is the main-group bank `2s37ak…`; if MarginFi pauses/obsoletes it, deposit/withdraw revert (fail-closed).
- Upgradeable program; upgrade authority should be the governance multisig in production.

## Tests
- [x] Rust unit (`cargo test -p ya-adapter-marginfi`): I80F48 value math vs on-chain snapshot + `mul_u128` boundaries.
- [x] Surfpool mainnet-fork (`tests/adapters/marginfi.spec.ts`): shared `runConformance` against real cloned MarginFi state + **EDGE: `current_value` == actual redeemed USDC, diff = 0**.
