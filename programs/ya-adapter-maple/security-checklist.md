# Security checklist — ya-adapter-maple

**Risk level: 🔴 Critical** — custodies syrupUSDC in a per-position `vault_authority` PDA, CPIs USDC
in/out of syrupUSDC via one Orca Whirlpool swap, and values the position with an **independent**
Chainlink exchange-rate feed (never the pool it trades in).

Derived from `safe-solana-builder` (Frank Castle). Applied:

## CPI safety (shared-base §5)
- [x] **One external CPI per op** into Orca Whirlpool (`whirLb…`, hardcoded `ORCA_ID`, asserted on the program account in `remaining_accounts`). deposit → `swap` USDC→syrupUSDC (`a_to_b=false`); withdraw → `swap` syrupUSDC→USDC (`a_to_b=true`). Built via the uniform `ya_interface::CpiCall` against the v1 `swap` 12-account order. Plus one SPL `transfer_checked` (owner↔vault); call tree stays at depth ≤ 4.
- [x] `invoke_signed` by the per-position `vault_authority` PDA for the swap and the vault→owner transfer; canonical bump stored on `Position` (`vault_authority_bump`) and reused (§2).
- [x] **No crank / no introspection:** the swap is instant and self-contained; no top-level refresh, no instruction-sysvar guard. CPI errors propagated with `?` (§5.6).
- [x] `sqrt_price_limit` set to the per-direction no-limit bound (`MIN_/MAX_SQRT_PRICE`); price protection is the explicit slippage floor (below), not a partial-fill limit.

## remaining_accounts validation (shared-base §1.4 / §9.3 / W011)
- [x] **Count + program + pool:** `remaining_accounts.len() >= 10` checked; `whirlpool == POOL` (the single hardcoded USDC↔syrupUSDC pool `6fteK…`) and `orca_program == ORCA_ID` asserted (`validate_orca`). A caller cannot route through a foreign pool or a spoofed program.
- [x] **Vault accounts (`validate_vault`):** the prefix `vault_token_account` (vault USDC) and the `vault_syrup` account are re-derived as this position's canonical PDAs (`[b"vault_usdc"|b"vault_syrup", position]`), and `vault_syrup`'s mint (bytes 0..32) is asserted == `SYRUP_MINT`. A caller cannot substitute a foreign vault or a non-syrup token account.
- [x] Token vaults A/B, the three tick arrays, and the pool oracle are forwarded to Orca, which re-derives + checks them against the (asserted) whirlpool internally — so they are bound to the pool, not free inputs.

## Token handling (shared-base §7 / §21.6)
- [x] **Balance-delta accounting:** deposit credits `shares` by the *actual* syrupUSDC balance delta on the vault syrup account (re-read before/after the swap); withdraw measures the *actual* USDC delta on the vault USDC account — never the requested amount (§21.6).
- [x] Transfers use SPL `transfer_checked` (decimals read from the mint, manual tag-12 CPI) — owner→vault on deposit (owner signs), vault→owner on withdraw (`vault_authority` signs).
- [x] Per-user `vault_authority` PDA with per-position `vault_usdc` / `vault_syrup` — no global vault (§5.8). Instant adapter: every withdraw settles in one call (§22.1); `settle_withdrawal` returns `NothingToSettle`.
- [x] Slippage enforced **twice**: as the Orca swap `other_threshold` *and* a post-swap `require!(received/redeemed >= min_*, SlippageExceeded)` against the measured delta. `withdraw(shares > position.shares)` reverts.

## Value math (`current_value`, shared-base §3 / §11 / §21)
- [x] **Priced off an independent oracle, not the pool.** value = `syrup_shares × Chainlink "SYRUPUSDC-USDC Exchange Rate" ÷ 10^decimals` — the position's redeemable USDC. **Never** the Orca spot quote and **never** 1:1, so a single swap into the same pool the adapter trades in cannot move the reported value (no self-referential manipulation vector). Execute against the market; price off the oracle.
- [x] **Fail-closed feed validation (`chainlink_value`):** feed key == `CHAINLINK_FEED`, feed owner == the OCR2 store (`CHAINLINK_OWNER`), `data.len() >= 232`, `decimals <= 18`, fresh timestamp (`now >= ts && now - ts <= MAX_STALE` = 3600s), and `answer (i128 @216) > 0` — any failure returns `OracleStale`. Offsets are byte-exact: `decimals@138`, `timestamp(u32)@208`, `answer(i128)@216`.
- [x] **Byte-exact, checked math:** `floor(shares · answer / 10^decimals)` via a hand-rolled 192-bit `mul_div_u64` (the product overflows u128); result bounded to `u64` (else `MathOverflow`). Pinned by a unit test (`cargo test -p ya-adapter-maple`).

## Account validation / lifecycle
- [x] Typed prefix (`Position` via `has_one = owner`, `has_one = base_mint`, canonical bump); `vault_authority` constrained by seeds + stored bump.
- [x] Adapter asserts the registry entry itself (`assert_active` → `load_adapter_entry`: owner+disc+canonical-PDA+program_id, then `status == Active` + `base_mint`) so direct (depth-1) calls are gated too (§7).
- [x] `initialize_position` is idempotent: `Position` guarded by `owner == default`; `vault_usdc` / `vault_syrup` are `init_if_needed` PDAs with `token::mint`/`token::authority` constraints.

## High-risk decisions / known limitations
- **Swap-and-hold, by necessity:** syrupUSDC is a Chainlink CCIP bridge token with no native synchronous Solana mint/redeem (its lending lives on Ethereum), so the only correct on-chain Solana primitive is a swap on the deepest USDC↔syrupUSDC Orca pool, holding syrupUSDC as the position. Entry/exit is therefore liquidity-constrained — real slippage, enforced via `min_amount_out`.
- Pool and feed are hardcoded. If the Orca pool is illiquid/halted the swaps revert; if the Chainlink answer is stale (>1h) or non-positive `current_value` reverts (`OracleStale`) rather than reporting a fabricated value — fail-closed in both cases.
- Upgradeable program; upgrade authority should be the governance multisig in production.

## Tests
- [x] Rust unit (`cargo test -p ya-adapter-maple`): `chainlink_value` math (`mul_div_u64` vs a live rate, incl. zero-shares and identity boundaries).
- [x] Surfpool mainnet-fork (`tests/adapters/maple.spec.ts`): the shared `runConformance` suite against real cloned Orca + Chainlink state + **EDGE: `current_value` == syrupUSDC balance × Chainlink rate, diff = 0** (asserted against the feed, not the pool quote).
