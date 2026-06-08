# The Yield Adapter Standard

One calling convention for depositing into, withdrawing from, and pricing any yield source on Solana. A non-custodial dispatcher routes three operations (`deposit`, `withdraw`, `current_value`) to small adapter programs, each of which translates the standard call into one venue's CPI. A governance-gated registry controls which adapters are live. A new venue is a new adapter plus a registry entry; the dispatcher and registry do not change.

## Reference implementation

Five adapters, green on a Surfpool mainnet-fork against real cloned protocol state.

```bash
git clone https://github.com/eternally-black/Yieldplex && cd Yieldplex && MAINNET_RPC_URL=<your-rpc> npm run check   # -> 59 passing on mainnet-fork
```

Build your own adapter: [BUILD_YOUR_OWN_ADAPTER.md](BUILD_YOUR_OWN_ADAPTER.md).

## Components

- **`ya-interface`** — a Rust crate (not a program) compiled into every adapter: the account layouts (`Position`, `WithdrawalTicket`), the manual-CPI primitive (`CpiCall`), the return-data view helpers, shared seeds, errors, and events. One crate, so every position has the same layout and one decoder reads them all.
- **`ya-registry`** — the on-chain record of which adapters exist and their status. Governance-gated.
- **`ya-dispatcher`** — the router. Checks the registry, forwards the standard call. Holds no funds.
- **adapters** — `ya-adapter-{kamino,marginfi,jupiter-jlp,maple,drift-if}`. Each owns the CPI into one venue and custodies the position under its own PDAs.

```
   caller (vault / router / agent)
        ▼
   ya-dispatcher
        ▼
   ya-adapter-X
        ▼
   Kamino / MarginFi / Jupiter / Orca / Drift
        ▼
   Position PDA
```

Funds and positions live under each adapter's PDAs. The dispatcher is a router and a safety gate, not a custodian; an adapter can also be called directly with the same instructions, but routing through the dispatcher is what enforces the registry check.

## Instructions

Every adapter implements these five. The dispatcher exposes four as routes (`route_deposit`, `route_withdraw`, `route_settle_withdrawal`, `route_current_value`); `initialize_position` runs once per position. Amounts and shares are in the asset's smallest unit (USDC = 6 decimals).

| Instruction | Effect |
|---|---|
| `initialize_position` | Creates the `Position` PDA for an (owner, base_mint) pair and its vault authority. One position per owner per base mint. |
| `deposit` | Moves `amount` of the base asset from the owner into the venue; records the resulting share balance. |
| `withdraw` | Starts an exit. Instant venues redeem and pay out here. Cooldown venues open a `Pending` ticket and move nothing. |
| `settle_withdrawal` | Completes a pending exit once `now >= unlock_ts`. Instant venues do not need it. |
| `current_value` | Live read of the position's worth in base-asset units, returned via return data. |

## Account prefix

CPI cannot pass a variable account struct, so the standard fixes a 9-account prefix (indices 0–8) common to every instruction, then lets each adapter read venue-specific accounts as an opaque tail.

| # | Account | Notes |
|---|---|---|
| 0 | `position` | writable; the `Position` PDA |
| 1 | `vault_authority` | signs the position's protocol CPIs; owns its token accounts |
| 2 | `base_mint` | the deposit asset |
| 3 | `vault_token_account` | writable |
| 4 | `owner` | signer |
| 5 | `owner_token_account` | writable; source of deposits, destination of withdrawals |
| 6 | `registry_entry` | read for the Active and base-mint checks |
| 7 | `token_program` | |
| 8 | `system_program` | |

Accounts after index 8 are the venue tail (reserves, banks, pool custodies, oracles, the venue program). The adapter validates the tail by count, owner, type, and order, returning `InvalidRemainingAccounts` on mismatch. The dispatcher forwards the tail untouched, which is why a new adapter needs no dispatcher change.

PDA seeds (shared, so derivation is identical everywhere):

- `Position` = `["position", owner, base_mint]`
- `vault_authority` = `["vault_authority", position]`
- `WithdrawalTicket` = `["ticket", position]` (at most one open ticket per position)

## State accounts

Generated for each adapter by `declare_ya_accounts!`, so layouts are byte-identical and one SDK decoder reads any adapter's position.

`Position` — 130 bytes (incl. 8-byte discriminator):

| Field | Type | Meaning |
|---|---|---|
| `owner` | `Pubkey` | position owner |
| `base_mint` | `Pubkey` | deposit asset |
| `adapter` | `Pubkey` | owning adapter program |
| `shares` | `u64` | venue share balance (cTokens / JLP / syrupUSDC / IF shares) |
| `cached_value` | `u64` | last computed base-asset value |
| `value_updated_ts` | `i64` | when `cached_value` was written |
| `bump` | `u8` | position bump |
| `vault_authority_bump` | `u8` | vault authority bump |

`WithdrawalTicket` — 74 bytes (incl. discriminator):

| Field | Type | Meaning |
|---|---|---|
| `position` | `Pubkey` | owning position |
| `shares` | `u64` | shares being withdrawn |
| `min_amount_out` | `u64` | slippage floor on payout |
| `unlock_ts` | `i64` | earliest `settle_withdrawal` time |
| `status` | `WithdrawalStatus` | `None` / `Pending` / `Settled` / `Cancelled` |
| `created_ts` | `i64` | request time |

## current_value

A live read, not a stored number. The adapter computes the position's redeemable value from the venue's current state and returns a `u64` via Solana return data (`report_value`); the dispatcher reads it back after the CPI (`read_returned_value`), so the caller gets it in the same transaction. Two rules:

- **Price the position, not an aggregate.** Value = the position's share balance times the venue's exchange rate, never a pool-wide total (e.g. a reserve's available liquidity).
- **Fail closed.** If a required price or exchange-rate source is stale or missing, return `OracleStale`.

## Withdrawals (two-phase)

`withdraw` starts every exit; `settle_withdrawal` finishes a deferred one.

- Instant venues (Kamino, MarginFi, Jupiter, Maple): `withdraw` redeems and pays out in the same call.
- Cooldown venues (Drift): `withdraw` writes a `Pending` `WithdrawalTicket` with an `unlock_ts` and moves no funds. `settle_withdrawal` completes the payout once `now >= unlock_ts`.
- Settling before `unlock_ts` returns `WithdrawalLocked`.
- Settling with no open ticket returns `NothingToSettle`.
- A venue-agnostic caller always calls `withdraw` then `settle_withdrawal`. For an instant venue the settle is a no-op.

## Registry and governance

Each adapter has a registry entry (program id, base mint, status). The dispatcher reads it on every route (prefix account 6) and forwards only if the adapter is `Active` and the base mint matches, else `AdapterNotActive` / `BaseMintMismatch`.

Status lifecycle: `Proposed` → `Active` (after governance approval) → `Paused` or `Deprecated`. All listing and status changes are governance-gated. Governance rotates in two steps (nominate, then accept), so one transaction cannot hand control to an unrecoverable address.

## Errors

| Error | Returned when |
|---|---|
| `AdapterNotActive` | the adapter is not `Active` in the registry |
| `BaseMintMismatch` | the call's base mint does not match the registry/adapter base mint |
| `SlippageExceeded` | output below `min_amount_out`, or position below `min_position_out` |
| `WithdrawalLocked` | settle attempted before `unlock_ts` |
| `NothingToSettle` | settle attempted with no pending ticket |
| `TicketAlreadyExists` | a withdrawal ticket already exists for the position |
| `OracleStale` | a required price / exchange-rate source is stale or unavailable (fails closed) |
| `InvalidRemainingAccounts` | the venue tail mismatches by count, owner, type, or order |
| `MathOverflow` | arithmetic overflow |
| `RegistryMismatch` | registry program id / adapter program id mismatch |

## Events

All events are keyed by `position`.

| Event | Fields |
|---|---|
| `PositionInitialized` | `owner`, `base_mint` |
| `Deposited` | `amount`, `value_after` |
| `WithdrawRequested` | `shares`, `unlock_ts` |
| `WithdrawSettled` | `amount_out` |
| `ValueReported` | `value` |

## Reference adapters

| Venue | Adapter | Integration | Value source |
|---|---|---|---|
| Kamino Lend (USDC) | `ya-adapter-kamino` | KLend V2 deposit/redeem; reserve refreshed as its own instruction | cToken balance × collateral exchange rate |
| MarginFi v2 (USDC) | `ya-adapter-marginfi` | `lending_account_deposit` / `_withdraw` on the USDC bank | asset shares × bank share value |
| Jupiter Perps JLP | `ya-adapter-jupiter-jlp` | `add_liquidity2` / `remove_liquidity2`; large account set via a lookup table | JLP balance × pool AUM ÷ JLP supply |
| Maple `syrupUSDC` | `ya-adapter-maple` | one direct Orca Whirlpool swap USDC↔syrupUSDC (swap-and-hold) | syrupUSDC balance × Chainlink exchange-rate feed |
| Drift Insurance Fund | `ya-adapter-drift-if` | insurance-fund stake; two-phase exit across the unstaking cooldown | IF shares ÷ total IF shares × fund balance |

Two notes:

- **Maple.** `syrupUSDC` is a Chainlink CCIP bridge token with no native Solana mint/redeem (the lending sits on Ethereum), so the Solana primitive is a swap on the deepest USDC↔syrupUSDC pool, holding syrupUSDC as the position. The swap executes at the pool price (guarded by `min_amount_out`); `current_value` reads an independent Chainlink exchange-rate feed instead, so the reported value cannot be moved by a single swap into the same pool the adapter trades against. Execute against the market, price off the oracle.
- **Drift.** The insurance-fund-stake instructions are commented out of Drift's deployed `#[program]`; the live program returns `InstructionFallbackNotFound` for those discriminators, so no caller can stake by CPI today. The adapter is written against the real layout and discriminators and conforms; it will work once those entry points are re-enabled upstream. Its two-phase lifecycle is exercised against a conformance stand-in, labeled as such wherever results are reported. Evidence: [docs/adapters/drift-if.md](adapters/drift-if.md).

## Conformance

A conforming adapter implements the five instructions with the 9-account prefix and shared seeds, custodies under the vault authority, prices the position as a fail-closed live view, and respects the registry gate. It passes the parametrized conformance suite (run against real venue state on a mainnet fork):

- position initializes once; re-initializing is harmless
- after `deposit`, `current_value` is within tolerance of the deposit
- `deposit` then `withdraw` round-trips funds back to the owner
- an impossible `min_amount_out` / `min_position_out` reverts with `SlippageExceeded`
- routing a `Paused` or `Proposed` adapter reverts with `AdapterNotActive`
- a mismatched base mint reverts with `BaseMintMismatch`
- a cooldown adapter opens a `Pending` ticket on `withdraw` and settles only after `unlock_ts`

Build steps and a worked example: [BUILD_YOUR_OWN_ADAPTER.md](BUILD_YOUR_OWN_ADAPTER.md). For an AI agent, the [agent skill](../skills/build-yield-adapter/SKILL.md) carries the same steps in an imperative, agent-ready form, so a coding agent can scaffold and ship a conforming adapter directly.

## Adding a venue

A new venue is a new adapter program that compiles in `ya-interface`, implements the five instructions, and is proposed then approved in the registry. The dispatcher routes to it with no redeploy, because it only reads the registry entry and forwards the standard call plus the opaque tail.
