# Security checklist — ya-dispatcher

**Risk level: 🔴 Critical** — routes value-bearing flows. **Non-custodial (router mode):** holds no
funds, owns no PDAs, uses `invoke` (never `invoke_signed`) → it cannot rug. Security rests on
registry gating + program-id/base-mint binding.

Derived from `safe-solana-builder` (Frank Castle). Applied:

## CPI safety (shared-base §5)
- [x] **No arbitrary CPI:** the target `adapter_program` is bound to the registry entry — `load_adapter_entry` verifies the entry is owned by `ya_registry`, has the right discriminator, is the **canonical `[b"adapter", adapter_program]` PDA**, and that `entry.program_id == adapter_program`. Only an Active, governance-approved program can be the CPI target.
- [x] `adapter_program` constrained `executable`.
- [x] `invoke` (not `invoke_signed`) — the end user's outer-tx signature propagates; the dispatcher never elevates privileges or signs as a PDA (§5.3, §5.7).
- [x] CPI error propagated with `?` (§5.6) — adapter failure reverts the whole tx.
- [x] One CPI into one program per route (CPI-depth budget): dispatcher→adapter is depth 2; the adapter does depth 3→4. Refresh/cranks are separate top-level ixs (never nested here).

## Gating (the security property)
- [x] `status == Active` required (`AdapterNotActive`) — covers pause/deprecate.
- [x] `base_mint == entry.base_mint` required (`BaseMintMismatch`).
- [x] Program-id bound via the canonical-PDA check (above) — no per-adapter special-casing; fully generic.
- [x] Tests: `paused_adapter_blocks_routing`, `base_mint_mismatch_blocks_routing`.

## remaining_accounts (shared-base §9.3)
- [x] Forwarded **opaquely** to the adapter preserving signer/writable flags. The dispatcher does not interpret them; the **adapter** is responsible for validating its own `remaining_accounts` (every reference adapter does). Documented contract.
- [x] Standard 9-account prefix forwarded with fixed, known mutability (position(w), vault_authority, base_mint, vault_token_account(w), owner(signer,w), owner_token_account(w), registry_entry, token_program, system_program).

## View / return data
- [x] `route_current_value` reads the adapter's returned `u64` via `get_return_data`, validates the returning program, and re-`set_return_data`s it (`OracleStale` if absent) so the dispatcher is itself view-callable. Test: `route_deposit_and_current_value`.

## High-risk decisions / known limitations
- Router mode only (no custody). A "vault mode" (dispatcher custodies + issues its own shares) is a documented future extension, intentionally out of scope — it would raise the risk profile and is not needed for the standard.
- The dispatcher trusts the adapter to enforce slippage/value math; it passes `min_*` through unchanged. Adapters are independently gated (they also assert the registry entry), so a direct (depth-1) adapter call is equally safe.
- Upgradeable program; upgrade authority should be the governance multisig in production.

## Tests (LiteSVM, `tests/dispatcher_tests.rs`)
- [x] Full route path registry → dispatcher → mock adapter (deposit + current_value via returnData).
- [x] Paused adapter blocks routing.
- [x] Base-mint mismatch blocks routing.
