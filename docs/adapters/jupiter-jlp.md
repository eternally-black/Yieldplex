# Jupiter JLP (Perps LP) USDC adapter

Provides USDC liquidity to the Jupiter Perps pool and holds the yield-bearing **JLP** token in the
position's vault. Instant (no cooldown). Adapter #3 — same vault shape as Kamino/MarginFi, but the
CPI revalues the whole pool, so it carries the most accounts.

- Program: `ya_adapter_jupiter_jlp` `9fqh4833yoSJoPzpsucHY2SbUafVfHcC48RLQhTTahsB`
- Protocol: Jupiter Perps `PERPHjGBqRHArX4DySjwM6UJHiR3sWAatqfdBS2qQJu`, pool `5BUwFW4nRbftYTDMbgxykoFWqWHPzahFSNAaaaJtVKsq`
- JLP mint: `27G8MtK7VtTcCHkpASjSDdkWWYfoqT6ggEuKidVJidD4`
- Base asset: USDC `EPjFW…Dt1v`

## How it works

| Standard op | Jupiter CPI (one, `invoke_signed` by `vault_authority`) |
|---|---|
| `initialize_position` | creates the Position, the vault USDC PDA token account `[b"vault_usdc", position]`, and the vault JLP PDA token account `[b"vault_jlp", position]`. |
| `deposit(amount, min_position_out)` | `transfer_checked` owner USDC → vault USDC, then `add_liquidity2(amount, min_jlp)` → JLP minted into the vault JLP account. `shares += actual JLP delta`. |
| `withdraw(shares, min_amount_out)` | `remove_liquidity2(shares, min_usdc)` → USDC into the vault, then `transfer_checked` vault → owner. Settles instantly. |
| `current_value()` | reads pool AUM + JLP supply, converts `shares` (JLP) → USDC at NAV; returned via return-data. |

`add_liquidity2` / `remove_liquidity2` take a single `params` struct, serialized field-by-field
(byte-identical to `borsh(struct)`) by the uniform manual-CPI layer — no `declare_program!`.

## Value is NAV (net asset value)

`current_value` is the pool's own net-asset-value math, not an oracle quote of JLP:

```
value (USDC) = floor( jlp_shares * aum_usd / jlp_total_supply )
```

`aum_usd` lives at a **dynamic Borsh offset** in the pool account (it follows a variable-length
`name` string and the custodies vector): `aum_off = 12 + name_len + 4 + n_custodies * 32`, then a
little-endian `u128`. The fork test asserts `current_value == jlp * aum_usd / supply, diff = 0`
against an independent off-chain read of the same fields.

## Accounts

Standard 9-account prefix, then `remaining_accounts` for deposit/withdraw (the underlying
`add/remove_liquidity2` CPI is a 24-account instruction; the prefix supplies the rest):

```
vault_jlp(w) · transfer_authority · perpetuals · pool(w) · usdc_custody(w)
usdc_doves_ag · usdc_doves_ag · usdc_custody_token(w) · jlp_mint(w) · event_authority · perps_program
+ all 5 custodies + all 5 doves_ag oracles
```

- both oracle slots are the **USDC custody's** doves_ag (`6Jp2xZ…`); the pool still requires *all 5*
  custodies + *all 5* doves_ag oracles trailing, because `add/remove_liquidity2` revalue every leg.
- `withdraw` prepends the `WithdrawalTicket`; `current_value` needs only `[pool, jlp_mint]`.

Verified accounts:
- perps program `PERPHjGBqRHArX4DySjwM6UJHiR3sWAatqfdBS2qQJu`, pool `5BUwFW4nRbftYTDMbgxykoFWqWHPzahFSNAaaaJtVKsq`
- PDAs: perpetuals `H4ND9aYttUVLFmNypZqLjZ52FYiGvdEB45GmwNoKEjTj` · transfer_authority `AVzP2GeRmqGphJsMxWoqjpUifPpCret7LqWhD8NWQK49` · event_authority `37hJBDnntwqhGbK7L6M1bLyvccj4u55CCUiLPdYkiqBN`
- USDC custody `G18jKKXQwBbrHeiK3C9MRXhkHsLHf7XgCSisykV46EZa` · custody_token `WzWUoCmtVv7eqAbU3BfKPU3fhLP6CXR8NCJH78UK9VS` · USDC doves_ag `6Jp2xZUTWdDD2ZyUPRzeMdc6AFQ5K3pFgZxk2EijfjnM`

The pool, custodies and oracles are validated (owner + discriminator) before any field read. The tx
fits a legacy transaction without an ALT, but the SDK still wires an Address Lookup Table for
robustness as the account list is the largest of the five adapters.

## Test

`tests/adapters/jlp.spec.ts` runs the shared `runConformance` suite against real cloned Jupiter state
on Surfpool, plus the EDGE `diff = 0` NAV proof. The doves_ag oracles have a 5s staleness window, so
the fork fixture refreshes their `publish_time` (i64 @177) via `surfnet_setAccount` before each op.
Run: `anchor test` or `bash scripts/fork-test.sh tests/adapters/jlp.spec.ts`. Pool ground truth:
`npx tsx scripts/inspect-jlp.ts` / `npx tsx scripts/verify-jup-perps.ts`.
