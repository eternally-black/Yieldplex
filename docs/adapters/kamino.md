# Kamino Lend (KLend) USDC adapter

Lends USDC into the Kamino main market and holds the yield-bearing **reserve collateral cTokens**
in the position's vault. Instant (no cooldown). This is the reference adapter the other four copy.

- Program: `ya_adapter_kamino` `BwyrWhHa86dCyRghZn9EDK2ZxfhpBH4tr5NVoBJ3hTs5`
- Protocol: Kamino KLend `KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD`, main market `7u3He…`
- Base asset: USDC `EPjFW…Dt1v`

## How it works

| Standard op | Kamino CPI (one, `invoke_signed` by `vault_authority`) |
|---|---|
| `deposit(amount, min_position_out)` | `transfer_checked` owner USDC → vault USDC, then `deposit_reserve_liquidity(amount)` → cTokens into the vault cToken account. `shares += actual cToken delta`. |
| `withdraw(shares, min_amount_out)` | `redeem_reserve_collateral(shares)` → USDC into vault, then `transfer_checked` vault → owner. Settles instantly. |
| `current_value()` | reads the Reserve and converts `shares` (cTokens) → USDC via the collateral exchange rate; returned via return-data. |

**Standalone reserve path, not the obligation path.** `deposit_reserve_liquidity` /
`redeem_reserve_collateral` mint/burn cTokens directly — no Kamino obligation, one CPI, lowest
depth. Source-verified CPI-callable on the main-market USDC reserve (the CPI guard sits only on the
obligation handlers). Both instructions **refresh the reserve internally in the same slot**, so the
adapter needs **no separate `refresh_reserve` and no oracle accounts**.

## Value is oracle-free

`current_value` does not read any price feed. The collateral exchange rate is pure token
accounting from the Reserve:

```
total_supply_sf = (total_available_amount << 60) + borrowed_amount_sf
                  - accumulated_protocol_fees_sf - accumulated_referrer_fees_sf - pending_referrer_fees_sf
value (USDC)    = floor( shares * total_supply_sf / collateral.mint_total_supply ) >> 60
```

The `_sf` fields are Kamino `U68F60` scaled fractions (value × 2⁶⁰). The `shares * total_supply_sf`
product exceeds u128, so it runs in a 192-bit `mul_div_u64`. This is **byte-exact** with what
`redeem_reserve_collateral` pays out — the fork test asserts `current_value == actual redeemed USDC,
diff = 0` (vs the field, which only checks value ≈ deposit). A competitor shipped Kamino value as
`reserve.available_liquidity()` (a pool aggregate) — wrong; we read the *position's* redeemable value.

## Accounts

Standard 9-account prefix, then `remaining_accounts` (deposit / withdraw):

```
vault_ctoken(w) · reserve(w) · lending_market · lending_market_authority
reserve_liquidity_supply(w) · reserve_collateral_mint(w) · instruction_sysvar · kamino_program
```
(withdraw prepends the `WithdrawalTicket`; `current_value` needs only `reserve`.)
`reserve_liquidity_mint` = the prefix `base_mint`; both token programs = the prefix `token_program`.

Verified reserve accounts (main market USDC reserve `D6q6wuQSrifJKZYpR1M8R4YawnLDtDsMmWM1NbBmgJ59`):
- `lending_market_authority` = `9DrvZvyWh1HuAoZxvYWMvkf2XCzryCpGgHqrMjyDWpmo` = PDA `[b"lma", market]`
- `reserve_liquidity_supply` = `Bgq7trRgVMeq33yt235zM2onQ4bRDBsY5EWiTetF4qw6`
- `reserve_collateral_mint` (cToken) = `B8V6WVjPxW1UGwVDfxH2d2r8SyT4cqn7dQRK6XneVa7D`

Every Kamino account passed in is validated against the (owner + discriminator + length checked)
Reserve, and the two vault accounts are re-derived as this position's canonical PDAs.

## Test

`tests/adapters/kamino.spec.ts` runs the shared `runConformance` suite against real cloned Kamino
state on Surfpool, plus the EDGE `diff = 0` proof. Run: `anchor test` (full suite) or
`bash scripts/fork-test.sh tests/adapters/kamino.spec.ts`. Value math is also unit-tested off-chain
against a live reserve snapshot: `bash scripts/test-rust.sh ya-adapter-kamino`.
