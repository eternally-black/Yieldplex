# Maple syrupUSDC USDC adapter

Swaps USDC into Maple's yield-bearing **syrupUSDC** through one Orca Whirlpool and holds it in the
position's vault (swap-and-hold). Instant (no cooldown). Adapter #4 — same vault shape as the others;
the protocol leg is a single Orca swap instead of a lending CPI.

- Program: `ya_adapter_maple` `Ck9mwpX9kAjycbtN7jhD3s9xdHzUS2dwuV43g3BuBnD`
- Protocol: Orca Whirlpool `whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc`, syrupUSDC/USDC pool `6fteKNvMdv7tYmBoJHhj1jx6rHcEwC6RdSEmVpyS613J`
- syrupUSDC mint: `AvZZF1YaZDziPY2RCK4oJrRVrbN3mTD9NL24hPeaZeUj`
- Base asset: USDC `EPjFW…Dt1v`

## How it works

| Standard op | Orca CPI (one, `invoke_signed` by `vault_authority`) |
|---|---|
| `initialize_position` | creates the Position, the vault USDC PDA `[b"vault_usdc", position]`, and the vault syrup PDA `[b"vault_syrup", position]`. |
| `deposit(amount, min_position_out)` | `transfer_checked` owner USDC → vault, then Whirlpool `swap` USDC → syrupUSDC into the vault syrup account. `shares += actual syrup delta`. |
| `withdraw(shares, min_amount_out)` | Whirlpool `swap` syrupUSDC → USDC into the vault, then `transfer_checked` vault → owner. Settles instantly. |
| `current_value()` | reads the Chainlink syrupUSDC rate and converts `shares` (syrup) → USDC; returned via return-data. |

The pool is **A = syrupUSDC / B = USDC**, so a deposit buys syrup (`a_to_b = false`) and a withdraw
sells it (`a_to_b = true`). Tick arrays are **direction-specific** — ascending for the buy,
descending for the sell — and the first tick array (`4yRC9N…`, current price) is shared by both.

## Value is the Chainlink rate, never the pool quote

`current_value` deliberately ignores the Orca quote (which moves with price impact and the swap fee)
and uses Maple's published exchange rate:

```
value (USDC) = floor( syrup_shares * chainlink_answer / 10^chainlink_decimals )
```

Read from the Chainlink feed `CpNyiFt84q66665Kx64bobxZuMgZ2EecrhAJs1HikS2T`: `answer` is an `i128`
@216, `decimals` a `u8` @138, the timestamp @208. The adapter **fails closed** if the answer is
`≤ 0` or the feed is stale — a swap-and-hold position must never report a fabricated value. The fork
test asserts `current_value == syrup * rate / 10^dec, diff = 0` against an independent read.

## Accounts

Standard 9-account prefix, then `remaining_accounts` for deposit/withdraw (the underlying Orca `swap`
v1 CPI is 12 accounts incl. the trailing `whirlpool_program`; the prefix supplies the rest):

```
vault_syrup(w) · pool(w) · vault_a(w) · vault_b(w) · tick_0(w) · tick_1(w) · tick_2(w)
oracle · whirlpool_program · chainlink_feed
```

- `withdraw` prepends the `WithdrawalTicket` and uses the **sell** tick arrays; `current_value` needs
  only `[chainlink_feed]`.
- the Chainlink feed travels in the deposit/withdraw remaining list as well (oracle-based slippage
  bound), and is the sole value account.

Verified accounts:
- vault A (syrup) `FM2RuqFYo9umA1yc5FyQn6pSDZJZ1MXAdaekJZ4dQCvi` · vault B (USDC) `Fw6Xr45rBBrXbWJd5ZbSg44kacrKRLef4rHkZ8gWC5Ab`
- pool `oracle` PDA `H7j5FQpwTUMwxrWeuyrLr5Z9oHsPFiaRqNaERVsuE1c8` = `[b"oracle", whirlpool]`
- buy tick arrays `4yRC9N… / AdLyWhs7… / AofDEAkf…` · sell tick arrays `4yRC9N… / 9qUH5rp6… / BQ95wDV5…`

The pool and vaults are validated (owner + discriminator + mint binding) before the swap, and the two
vault accounts are re-derived as this position's canonical PDAs.

## Test

`tests/adapters/maple.spec.ts` runs the shared `runConformance` suite against real cloned Orca +
Chainlink state on Surfpool, plus the EDGE `diff = 0` proof (value vs the Chainlink rate, not the
pool quote). Run: `anchor test` or `bash scripts/fork-test.sh tests/adapters/maple.spec.ts`. Feed
ground truth: `node scripts/inspect-chainlink.mjs`.
