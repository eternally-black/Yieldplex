# Security checklist — ya-adapter-jupiter-jlp

**Risk level: 🔴 Critical** — custodies JLP in the per-position `vault_authority` PDA, CPIs USDC↔JLP
via Jupiter Perps `add_liquidity2`/`remove_liquidity2`, and values the JLP position by pool NAV.

Derived from `safe-solana-builder` (Frank Castle). Applied:

## CPI safety (shared-base §5)
- [x] **One external CPI per op** into Jupiter Perps (`PERPH…`, hardcoded `PERPS_ID`, asserted on the program account passed in `remaining_accounts`). deposit → `add_liquidity2`; withdraw → `remove_liquidity2`. Built via the uniform `ya_interface::CpiCall` (the single `params` struct is serialized field-by-field — byte-identical to the borsh struct).
- [x] **`invoke_signed` by `vault_authority`** (the `owner`/`transfer_authority` source). USDC flows owner→vault→Jupiter (deposit) and Jupiter→vault→owner (withdraw) via `transfer_checked`.
- [x] CPI errors propagated with `?`. CPI-depth: dispatcher→adapter→Jupiter (→ token/oracle internally) ≤ 4.

## remaining_accounts validation (shared-base §1.4 / §9.3)
- [x] `pool == JLP_POOL`, `lp_token_mint == JLP_MINT`, protocol program == `PERPS_ID` asserted. The custody + its doves/pythnet oracle + custody_token_account are forwarded; Jupiter validates them against the pool/custody internally (and reverts on a wrong/foreign custody).
- [x] **Vault accounts:** prefix `vault_token_account` (vault USDC) and the `vault_jlp` account are re-derived as this position's canonical PDAs (`[b"vault_usdc"|b"vault_jlp", position]`).
- [x] `read_pool_aum` checks the Pool's owner + 8-byte discriminator + length, and parses `aum_usd` at its **dynamic** offset (the Pool is Borsh with a leading String+Vec) — never a hardcoded offset.

## Token handling / value (shared-base §7 / §21.6 / §11)
- [x] **Balance-delta accounting:** deposit credits `shares` by the actual JLP balance delta; withdraw measures the actual USDC delta — never the requested amount.
- [x] `transfer_checked` (decimals from the mint); per-user vault PDA (§5.8); every position has a withdrawal path (§22.1).
- [x] Slippage: `min_lp_amount_out` (deposit) and `min_amount_out` (withdraw) are passed **into** Jupiter (which enforces them) AND re-checked against the measured delta → `SlippageExceeded`.
- [x] **Value = NAV, the position's redeemable mark, not a pool aggregate:** `value = floor(jlp_balance × Pool.aum_usd / JLP_mint.supply)` (full 256-bit multiply; `aum_usd` is USD×1e6). The EDGE fork test asserts `current_value == the protocol's own NAV` (diff = 0). (A round-trip deposit→remove returns slightly less than the deposit because add/remove carry a pool fee — that fee is real liquidity-provision cost, not a value error; the conformance `value ≈ deposit` check uses a fee-sized tolerance.)
- [x] **Oracle freshness (§11, C2/C5):** Jupiter rejects stale `doves`/`pythnet` price accounts; on a Surfpool fork the cloned oracle may be stale, so the fork test refreshes it (`surfnet_setAccount`/clock-align) — documented in `tests/fork/FIXTURES.md`; fails closed on the real chain. `current_value` itself is oracle-independent for the JLP-count→NAV math (it reads `aum_usd`, which Jupiter keeps fresh).

## Account validation / lifecycle
- [x] Typed prefix (`Position`, `has_one`, canonical bumps); `vault_authority` seed-constrained; adapter asserts the registry entry (`load_adapter_entry`).
- [x] `initialize_position` idempotent (Position guarded by `owner == default`; vault USDC + vault JLP `init_if_needed`).

## High-risk decisions / known limitations
- Liquidity is provided to the **USDC custody** of the JLP pool; the position is JLP (exposed to the basket's price, by design — JLP is the yield product). `current_value` is the JLP NAV in USD, the standard JLP valuation.
- The 14-account add/remove + the standard prefix exceed the legacy-tx account budget → the SDK/tests build a **versioned tx with an Address Lookup Table** (static Jupiter accounts: transfer_authority, perpetuals, pool, custody, oracles, event_authority, program).
- Upgradeable program; upgrade authority should be the governance multisig in production.

## Tests
- [x] Rust unit (`cargo test -p ya-adapter-jupiter-jlp`): NAV math vs on-chain pool snapshot.
- [x] Surfpool mainnet-fork (`tests/adapters/jlp.spec.ts`): shared `runConformance` against real cloned Jupiter state (ALT-built txs) + **EDGE: `current_value` == pool NAV, diff = 0**.
