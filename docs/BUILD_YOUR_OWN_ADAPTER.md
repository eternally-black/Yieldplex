# Build your own adapter

An adapter is one program that implements the standard's five instructions and turns them into a single CPI into one venue. The dispatcher and the registry never change; you ship a program and a registry entry. By the end of this guide your adapter passes the same conformance suite the five reference adapters pass.

Read [SPEC.md](SPEC.md) first for the interface (the 9-account prefix, the instructions, `Position` / `WithdrawalTicket`, the error and event sets). This guide is the build steps for a human. If you are an AI agent, or want to hand the job to one, use the [agent skill](../skills/build-yield-adapter/SKILL.md) instead: it carries these same steps in an imperative, agent-ready form, so a coding agent scaffolds and ships a conforming adapter in one pass.

## 1. Prerequisites

- **Toolchain:** Anchor `1.0.2`, Solana `3.1.10`, surfpool `1.3.1`, Rust edition 2024. `surfpool` is the binary; `surfnet` is the local mainnet-fork validator it runs.
- **Dependencies:** add `ya-interface` and `ya-registry` as path deps. `ya-interface` gives you the account layouts, the CPI primitive, the view helpers, the seeds, the errors, and the events.
- **A mainnet RPC** in `.env` as `MAINNET_RPC_URL`. The fork clones live venue state through it on first touch, so your tests run against the real Kamino/Drift/etc. accounts.
- **A program keypair.** Your `declare_id!` must match the key you deploy with.

## 2. The skeleton

Every adapter has the same shape. The fastest start is to copy [`programs/ya-mock-adapter`](../programs/ya-mock-adapter), which implements all five instructions with a mock 1:1 body and no token movement, then swap each body for a real CPI.

```rust
use anchor_lang::prelude::*;
use ya_interface::{constants::seeds, report_value, Deposited, PositionInitialized,
                   ValueReported, WithdrawSettled, YaError};
use ya_registry::AdapterStatus;

declare_id!("<your program id>");

ya_interface::declare_ya_accounts!(); // generates Position + WithdrawalTicket, owned by this program

#[program]
pub mod ya_adapter_myvenue {
    use super::*;

    pub fn initialize_position(ctx: Context<InitializePosition>) -> Result<()> { /* ... */ }
    pub fn deposit(ctx: Context<Op>, amount: u64, min_position_out: u64) -> Result<()> { /* ... */ }
    pub fn withdraw(ctx: Context<WithdrawOp>, shares: u64, min_amount_out: u64) -> Result<()> { /* ... */ }
    pub fn settle_withdrawal(ctx: Context<Op>, min_amount_out: u64) -> Result<()> { /* ... */ }
    pub fn current_value(ctx: Context<Op>) -> Result<()> { /* ... */ }
}
```

`declare_ya_accounts!()` emits the `Position` and `WithdrawalTicket` structs (identical layout across all adapters, so one SDK decoder reads them) plus the account-context structs that carry the 9-account prefix. You do not redefine the prefix; you receive it.

## 3. Implement the five instructions

| Instruction | Must do | Result |
|---|---|---|
| `initialize_position` | If `position.owner == default`, set `owner` / `base_mint` / `adapter` / bumps. Must be safe to call twice. | `Position` PDA exists; emit `PositionInitialized` |
| `deposit(amount, min_position_out)` | `assert_active`; CPI into the venue to deposit `amount`; record the resulting shares; require `out >= min_position_out`. | shares on position; emit `Deposited { amount, value_after }` |
| `withdraw(shares, min_amount_out)` | `assert_active`. Instant: redeem `shares` via CPI, pay the owner, write a `Settled` ticket. Cooldown: write a `Pending` ticket with `unlock_ts`, move nothing. Require `amount_out >= min_amount_out`. | emit `WithdrawSettled` (instant) or `WithdrawRequested` (cooldown) |
| `settle_withdrawal(min_amount_out)` | Instant: return `NothingToSettle`. Cooldown: require `now >= unlock_ts` (else `WithdrawalLocked`), redeem, pay the owner, mark the ticket `Settled`. | funds to owner; emit `WithdrawSettled` |
| `current_value()` | `assert_active`; read venue state, compute the position's value, `report_value(value)`, cache it. | value via return data; emit `ValueReported { value }` |

Gate every entry point yourself so a direct (depth-1) call is checked too, not only calls through the dispatcher:

```rust
fn assert_active(registry_entry: &AccountInfo, base_mint: &Pubkey) -> Result<()> {
    let entry = ya_registry::load_adapter_entry(registry_entry, &crate::ID)?;
    require!(entry.status == AdapterStatus::Active, YaError::AdapterNotActive);
    require_keys_eq!(entry.base_mint, *base_mint, YaError::BaseMintMismatch);
    Ok(())
}
```

## 4. The venue tail

The 9 prefix accounts (indices 0–8) are fixed and you receive them. Everything your venue needs goes after index 8 as `remaining_accounts`, in an order you define: reserves, banks, pool custodies, oracles, the venue program itself.

- Validate the tail by count, owner, key, type, and order. Return `InvalidRemainingAccounts` on any mismatch. Do not trust position; check it.
- Document the order in a comment so integrators can build the call. The reference adapters do this, e.g. Maple's tail is `0 vault_syrup 1 whirlpool 2 token_vault_a 3 token_vault_b 4..6 tick_arrays 7 oracle 8 orca_program 9 chainlink_feed`.
- The dispatcher forwards the tail untouched. It does not need to understand it. That is what lets your adapter ship with zero dispatcher changes.

## 5. The CPI

One adapter does one CPI into one venue program per operation. Build it by hand with `CpiCall`; there is no IDL-generated client to keep in sync.

```rust
ya_interface::CpiCall::global(venue_program_id, "deposit_reserve_liquidity")
    .arg(&amount)
    .account(reserve,            false, true)
    .account(vault_token_account, false, true)
    .account(vault_authority,     true,  false)
    // ... the rest of the venue's account list, in its order ...
    .invoke_signed(
        account_infos,
        &[&[seeds::VAULT_AUTHORITY, position.key().as_ref(), &[vault_authority_bump]]],
    )?;
```

- The 8-byte discriminator is `anchor_discriminator("global", ix_name)`. Cross-check it against the venue's IDL before trusting it; a wrong discriminator surfaces as `InstructionFallbackNotFound`.
- `vault_authority` signs via `invoke_signed` with its PDA seeds. It owns the vault token accounts and any venue sub-accounts.
- **Depth.** Stay inside the CPI budget: one direct CPI into the venue. For a swap, target one specific pool, never an aggregator (an aggregator routes through several programs and blows the depth).
- **Refreshes and cranks** (e.g. Kamino `refresh_reserve`) go as a separate top-level instruction, not nested inside your CPI. Pass them as `preInstructions` in the test.

## 6. current_value, done right

This is the instruction adapters get wrong, so it has its own rules.

- **Price the position, not an aggregate.** Value = the position's share balance times the venue's exchange rate. Never a pool-wide total like a reserve's available liquidity. Reading the aggregate compiles, passes a smoke test, and reports a number that can be orders of magnitude off.
- **Fail closed.** If the value depends on a price or exchange-rate source and it is stale or missing, return `OracleStale`. Do not guess.
- **Return it through return data.** Call `report_value(value)` (a `u64` in base-asset units) and cache it on the position. The dispatcher reads it back with `read_returned_value` after the CPI, so a caller gets the number in the same transaction.
- **Execution price and valuation price are different questions.** If your venue is a swap (Maple), execute the swap at the pool price with a `min_amount_out` guard, but value the position off an independent oracle. Pricing a position off the same pool it trades in gives you a view a single swap can move, which is a manipulation vector, not a valuation. Execute against the market; price off the oracle.

## 7. Run the conformance suite

Every adapter passes the same parametrized suite. Inside a `describe(...)`, after you register and initialize your adapter, call `runConformance(() => cfg)`:

```ts
describe("ya-adapter-myvenue", () => {
  before(async () => { await registerAndApprove(MYVENUE_ID, USDC_MINT); });
  runConformance(() => ({
    label: "myvenue",
    adapter: myvenueProgram,
    baseMint: USDC_MINT,
    depositAmount: new BN(20_000_000),     // 20 USDC
    toleranceBps: 5,                        // rounding + fees
    isInstant: true,                        // false + cooldownSeconds for two-phase
    initPosition: async () => { /* idempotent: create Position + sub-accounts */ },
    depositRemaining: () => [ /* your venue tail */ ],
    withdrawRemaining: () => [ /* tail, must include the withdrawal ticket */ ],
    valueRemaining: () => [ /* tail needed to price */ ],
    preInstructions: async () => [ /* refresh/crank, if any */ ],
    vaultTokenAccount: () => vaultUsdcPda,  // for token-moving adapters
    ownerTokenAccount: () => ownerUsdcAta,
  }));
});
```

What the suite checks:

| Check | Asserts |
|---|---|
| init idempotent | calling `initialize_position` twice does not throw |
| deposit then value | after `deposit`, `current_value` is within `toleranceBps` of the deposit |
| round-trip | `deposit` then `withdraw` returns the funds to the owner |
| slippage | an impossible `min_amount_out` / `min_position_out` reverts with `SlippageExceeded` |
| registry gate | routing a `Paused` / `Proposed` adapter reverts with `AdapterNotActive` |
| base mint | a mismatched base mint reverts with `BaseMintMismatch` |
| two-phase | a cooldown adapter opens a `Pending` ticket on `withdraw`, settles only after `unlock_ts` |

Run it against the fork:

```bash
bash scripts/fork-test.sh tests/adapters/myvenue.spec.ts
```

`fork-test.sh` starts surfnet forking from `MAINNET_RPC_URL`, deploys your built program, and runs the spec against real cloned venue state.

## 8. Register it

Your adapter is live once it is in the registry:

- propose the adapter (program id + base mint),
- governance approves it to `Active`.

The dispatcher routes to it immediately, with no redeploy, because it only reads the registry entry and forwards the standard call plus your opaque tail. Confirm a devnet deployment with `npm run verify:devnet` (asserts the programs are executable and the adapter is `Active`).

## 9. Gotchas

These are the failures that actually came up building the reference set.

| Symptom | Cause | Fix |
|---|---|---|
| `InstructionFallbackNotFound` (101) on the venue CPI | wrong discriminator, or the venue does not export that instruction by CPI | cross-check `anchor_discriminator("global", name)` against the IDL; confirm the instruction is actually callable on the deployed program |
| Kamino deposit reverts only via CPI | using the old combined KLend instructions | use the KLend **V2** mutation instructions |
| "reserve stale" / oracle too old, only on the fork | the cloned oracle's `publish_time` is older than the advancing fork clock | bump it with `surfnet_setAccount` in the test and list it in `FIXTURES.md`; production code fails closed |
| Jupiter `add_liquidity2` fails, tx too large | the JLP account set is ~24 accounts | build the tx with an Address Lookup Table |
| `current_value` wildly wrong | reading a venue aggregate instead of the position | value = shares × exchange rate |
| value diff is a few lamports, not 0 | lazy interest accrued between the value read and the redemption | pin both to the same instant for the diff=0 test (`FIXTURES.md`), or assert within tolerance |
| swap CPI exceeds depth | routing through an aggregator | swap on one direct pool |

## 10. Hello Yield: empty folder to green

The shortest path to a conforming adapter, no real venue:

1. Copy [`programs/ya-mock-adapter`](../programs/ya-mock-adapter) to `programs/ya-adapter-hello` and set a fresh `declare_id!`. It already implements all five instructions with a mock 1:1 body and gates on the registry.
2. `anchor build`.
3. Write `tests/adapters/hello.spec.ts`: register + approve the adapter, then `runConformance` with `isInstant: true`, a small `toleranceBps`, and no venue tail (the mock needs none).
4. `bash scripts/fork-test.sh tests/adapters/hello.spec.ts`. It passes: the mock satisfies the whole interface.

Then make it real, one method at a time: replace each mock body with a single `CpiCall` into your venue (§5), add the venue tail (§4) and the value math (§6), set `isInstant`/`cooldownSeconds` to match the venue, and re-run the suite after each change. When all checks stay green against real cloned state, propose it to the registry and you are done.
