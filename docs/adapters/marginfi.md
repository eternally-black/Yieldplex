# MarginFi v2 USDC adapter

Lends USDC into the MarginFi v2 main group and holds the position as `asset_shares` inside a
program-owned `marginfi_account`. Instant (no cooldown). Adapter #2 — same shape as the Kamino one.

- Program: `ya_adapter_marginfi` `36CgQYZFxZQHzyMrn3NJRXR9jsVoYH44WitqGohoBGoi`
- Protocol: MarginFi v2 `MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA`, main group `4qp6Fx…`
- USDC bank: `2s37akK2eyBbp8DZgCm7RtsaEz8eJP3Nxd4urLHQv7yB`

## How it works

| Standard op | MarginFi CPI (one, `invoke_signed` by `vault_authority`) |
|---|---|
| `initialize_position` | creates the Position, vault USDC PDA token account, and (once) the `marginfi_account` PDA via `marginfi_account_initialize` (authority = `vault_authority`). |
| `deposit(amount, min_position_out)` | `transfer_checked` owner USDC → vault, then `lending_account_deposit(amount, None)`. `shares` tracks deposited principal. |
| `withdraw(shares, min_amount_out)` | `lending_account_withdraw(amount, withdraw_all)` → vault, then `transfer_checked` vault → owner. `withdraw_all` when withdrawing the whole principal. Settles instantly. |
| `current_value()` | our `asset_shares` × `Bank.asset_share_value` → redeemable USDC (return-data view). |

Unlike Kamino (cToken balance), the MarginFi position lives **inside** a `marginfi_account` as
`asset_shares` (I80F48). The adapter makes that account a **program-owned PDA**
`[b"marginfi_account", position]` whose authority is the `vault_authority` PDA, so one PDA controls
creation and every deposit/withdraw via `invoke_signed`. Deposit/withdraw **accrue interest
internally in-slot** — no separate crank, no oracle for deposit.

## Value

```
value (USDC) = floor( asset_shares × asset_share_value ) = (asset_shares_bits × asv_bits) >> 96
```
Both fields are MarginFi `WrappedI80F48` (value × 2⁴⁸); the product exceeds u128 so it runs through a
full 256-bit multiply. This is **byte-exact** with marginfi's own `get_asset_amount(shares).floor()`
that `lending_account_withdraw` pays out — the fork test asserts `current_value == actual redeemed
USDC, diff = 0` (pinning the surfnet clock so both observations accrue to the same instant, since
MarginFi accrues interest lazily). We read the **position's** shares, not a bank aggregate.

## Accounts

Standard 9-account prefix, then `remaining_accounts`:
- **deposit:** `marginfi_account(w) · marginfi_group · bank(w) · bank_liquidity_vault(w) · marginfi_program`
- **withdraw:** (after the `WithdrawalTicket`) `marginfi_account(w) · marginfi_group · bank(w) · bank_liquidity_vault_authority(w) · bank_liquidity_vault(w) · oracle · marginfi_program`
- **current_value:** `marginfi_account · bank`

The health-check oracle for the USDC bank is `oracle_keys[0]` = `Dpw1EAVrSB1ibxiDQyTAW6Zip3J4Btk2x4SgApQCeFbX`
(Pyth push). `bank_liquidity_vault_authority` = PDA `[b"liquidity_vault_auth", bank]`. The bank is
validated (owner + discriminator + `mint`/`group`) before any field read.

## Test

`tests/adapters/marginfi.spec.ts` — shared `runConformance` on real cloned MarginFi state + the EDGE
`diff = 0` proof. Run: `anchor test` or `bash scripts/fork-test.sh tests/adapters/marginfi.spec.ts`.
Value math is unit-tested off-chain: `bash scripts/test-rust.sh ya-adapter-marginfi`. Bank ground
truth: `node scripts/inspect-marginfi.mjs`.
