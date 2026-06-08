# Security checklist — ya-adapter-kamino

**Risk level: 🔴 Critical** — custodies the position (Kamino collateral cTokens) in a per-position
`vault_authority` PDA, CPIs USDC in/out of Kamino KLend, and computes redeemable value. Non-custodial
*dispatcher* still applies; the adapter is the custody boundary, so the rules below are enforced here.

Derived from `safe-solana-builder` (Frank Castle). Applied:

## CPI safety (shared-base §5)
- [x] **One external CPI per op** into Kamino KLend (`KLend2g3…`, hardcoded `KAMINO_ID`, asserted on the program account passed in `remaining_accounts`). deposit → `deposit_reserve_liquidity`; withdraw → `redeem_reserve_collateral`. Both **self-refresh the reserve internally in-slot** (source-verified), so no separate top-level `refresh_reserve` and no oracle accounts are needed — keeping the call tree at depth ≤ 4.
- [x] **No instruction-introspection block:** the standalone reserve instructions carry no `check_cpi!`/`check_refresh_ixs!` guard (only the obligation handlers do), so the manual CPI is permitted. The vestigial `instruction_sysvar_account` is forwarded but unused by Kamino for these two ixs.
- [x] `invoke_signed` by the per-position `vault_authority` PDA for every protocol CPI; canonical bump stored on `Position` (`vault_authority_bump`) and reused (§2).
- [x] CPI errors propagated with `?` (§5.6).
- [x] Exact IDL account order built via the uniform `ya_interface::CpiCall` (deposit and redeem orders differ — verified against the vendored IDL + klend source).

## remaining_accounts validation (shared-base §1.4 / §9.3 / W011)
- [x] **Reserve:** owner == `KAMINO_ID`, 8-byte `Reserve` discriminator, and length checked **before** reading any field (offset reads, no full deserialize → no large stack frame, §25).
- [x] The reserve is the **single source of truth**: `lending_market`, `reserve_liquidity_supply`, `reserve_collateral_mint`, and `base_mint` (== `reserve.liquidity_mint`) passed in `remaining_accounts` are each asserted equal to the values decoded from the (owner+disc-checked) reserve. A caller cannot substitute a foreign market/supply/mint.
- [x] **Vault accounts:** the prefix `vault_token_account` (vault USDC) and the `vault_ctoken` account are re-derived as this position's canonical PDAs (`[b"vault_usdc"|b"vault_ctoken", position]`) and the cToken account's mint is asserted == `reserve.collateral_mint`.
- [x] `lending_market_authority` is forwarded and validated by Kamino itself (it re-derives `[b"lma", lending_market]`); the seed is documented + asserted in the fork test.

## Token handling (shared-base §7 / §21.6)
- [x] **Balance-delta accounting:** deposit credits `shares` by the *actual* cToken balance delta on the vault cToken account (re-read after CPI); withdraw measures the *actual* USDC delta — never the requested amount (§21.6).
- [x] Transfers use SPL `transfer_checked` (decimals read from the mint) — owner→vault on deposit (owner signs), vault→owner on withdraw (`vault_authority` signs).
- [x] Per-user `vault_authority` PDA — no global vault (§5.8). Every position has a withdrawal path (§22.1).
- [x] Slippage enforced: `min_position_out` (deposit) / `min_amount_out` (withdraw) → `SlippageExceeded`.

## Value math (`current_value`, shared-base §3 / §11 / §21)
- [x] **Oracle-free:** value = cToken collateral exchange rate from pure token accounting (available + borrowed − protocol/referrer/pending fees) ÷ cToken supply. No price feed → cannot be poisoned by a stale/again oracle. (The exchange rate is the same one `redeem_reserve_collateral` pays out.)
- [x] **Byte-exact with the chain:** `floor(shares · total_supply_sf / mint_total_supply) >> 60` in a hand-rolled 192-bit `mul_div_u64` (the product overflows u128). Matches klend's `BigFraction` path; verified against klend master over 17M adversarial cases and pinned by a unit test to a live on-chain snapshot.
- [x] Checked arithmetic throughout (`checked_add/sub`, overflow→`MathOverflow`); INITIAL 1:1 rate handled when the reserve is empty.

## Account validation / lifecycle
- [x] Typed prefix (`Position` via `has_one = owner, has_one = base_mint`, canonical bump); `vault_authority` constrained by seeds.
- [x] Adapter asserts the registry entry itself (`load_adapter_entry` → owner+disc+canonical-PDA+program_id, then `status == Active` + `base_mint`) so direct (depth-1) calls are gated too (§7).
- [x] `init_if_needed` guarded by the `owner == default` check in `initialize_position` (idempotent, §anchor 2.4).

## High-risk decisions / known limitations
- **Standalone reserve path (not obligation):** deposits mint reserve collateral cTokens directly to the vault (no Kamino obligation). Source-verified CPI-callable on the main-market USDC reserve, and the simplest path with the lowest depth. If Kamino ever sets the reserve `emergency_mode`/`Obsolete`/`block_ctoken_usage`, deposit/withdraw revert (fail-closed).
- `current_value` reflects the reserve's last in-slot refresh; for byte-exact equality with a redemption, read it in the same slot the redeem refreshes (the EDGE fork test redeems-all in-slot to prove diff = 0).
- Upgradeable program; upgrade authority should be the governance multisig in production.

## Tests
- [x] Rust unit (`cargo test -p ya-adapter-kamino`): value math vs on-chain snapshot + `mul_div_u64` boundaries.
- [x] Surfpool mainnet-fork (`tests/adapters/kamino.spec.ts`): the shared `runConformance` suite against real cloned Kamino state + **EDGE: `current_value` == actual redeemed USDC, diff = 0**.
