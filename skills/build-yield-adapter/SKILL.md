---
name: build-yield-adapter
description: Use when building, scaffolding, or modifying a yield adapter that conforms to the Yield Adapter Standard in this repo. The standard is an Anchor program exposing initialize_position / deposit / withdraw / settle_withdrawal / current_value, custodying a Position under PDAs, routed by ya-dispatcher behind a governance-gated ya-registry, and tested on a Surfpool mainnet fork. Trigger when asked to add a new venue or protocol adapter, wire a protocol CPI behind the standard interface, make an adapter pass the conformance suite, or fix an adapter that fails it.
---

# Build a Yield Adapter

Build one Anchor program that implements the five standard instructions and turns them into a single CPI into one venue. The dispatcher and registry do not change. The adapter is done when it passes the conformance suite against real cloned mainnet state and is `Active` in the registry.

## Read first

Load these from the repo before writing code. Do not skip; the standard's invariants live here.

- `docs/SPEC.md` — the interface: the 9-account prefix, the five instructions, `Position` / `WithdrawalTicket` layouts, the error and event sets.
- `programs/ya-mock-adapter/src/lib.rs` — the reference skeleton. Every adapter is this shape with the mock body replaced by a real CPI.
- `docs/adapters/<venue>.md` — if a doc exists for the target venue, read it for venue-specific accounts, discriminators, and value math.
- `tests/conformance/runConformance.ts` — the `ConformanceConfig` interface you will fill.
- `idls/CPI-META.md` and `idls/<venue>.json` — the venue's real discriminators and account orders.

## Invariants (do not violate)

- The 9-account prefix (indices 0–8) is fixed and you receive it. Do not redefine it. Venue accounts go after index 8 as an opaque tail.
- `current_value` prices the position (`shares × exchange rate`), never a venue aggregate. It fails closed (`OracleStale`) on a stale or missing price source, and returns the `u64` via `report_value`.
- One adapter does one direct CPI into one venue program per operation. Swaps use one specific pool, never an aggregator. Refreshes and cranks are separate top-level instructions, never nested.
- Every instruction calls `assert_active` so direct (depth-1) calls are gated, not only calls through the dispatcher.
- Build CPIs by hand with `CpiCall`. There is no IDL-generated client. Cross-check every discriminator against the venue IDL.
- Never present a stand-in or mock run as a live-protocol pass. If a venue path is not executable (e.g. an instruction is not exported by the deployed program), say so and prove it.

## Steps

### 1. Scaffold

Copy `programs/ya-mock-adapter` to `programs/ya-adapter-<venue>`. Set a fresh `declare_id!` (the key must match the deploy key). Keep:

```rust
ya_interface::declare_ya_accounts!(); // Position + WithdrawalTicket, owned by this program
```

### 2. Implement the five instructions

Signatures are fixed:

```rust
pub fn initialize_position(ctx: Context<InitializePosition>) -> Result<()>;
pub fn deposit(ctx: Context<Op>, amount: u64, min_position_out: u64) -> Result<()>;
pub fn withdraw(ctx: Context<WithdrawOp>, shares: u64, min_amount_out: u64) -> Result<()>;
pub fn settle_withdrawal(ctx: Context<Op>, min_amount_out: u64) -> Result<()>;
pub fn current_value(ctx: Context<Op>) -> Result<()>;
```

Behavior:

- `initialize_position`: if `position.owner == Pubkey::default()`, set `owner`, `base_mint`, `adapter = crate::ID`, and both bumps; emit `PositionInitialized`. Calling twice must not error.
- `deposit`: `assert_active`; CPI deposit `amount` into the venue; set `position.shares` to the resulting balance; `require!(out >= min_position_out, YaError::SlippageExceeded)`; emit `Deposited { amount, value_after }`.
- `withdraw`: `assert_active`. Instant venue: redeem `shares` via CPI, pay the owner, write a `Settled` ticket. Cooldown venue: write a `Pending` `WithdrawalTicket` with `unlock_ts`, move no funds. Enforce `min_amount_out`. Emit `WithdrawSettled` (instant) or `WithdrawRequested` (cooldown).
- `settle_withdrawal`: instant venue returns `YaError::NothingToSettle`. Cooldown venue: `require!(now >= ticket.unlock_ts, YaError::WithdrawalLocked)`, redeem, pay the owner, set the ticket `Settled`, emit `WithdrawSettled`.
- `current_value`: `assert_active`; read venue state; compute the position value; `report_value(value)`; cache on the position; emit `ValueReported { value }`.

### 3. Gate every instruction

Copy this and call it at the top of `deposit` / `withdraw` / `settle_withdrawal` / `current_value`:

```rust
fn assert_active(registry_entry: &AccountInfo, base_mint: &Pubkey) -> Result<()> {
    let entry = ya_registry::load_adapter_entry(registry_entry, &crate::ID)?;
    require!(entry.status == ya_registry::AdapterStatus::Active, YaError::AdapterNotActive);
    require_keys_eq!(entry.base_mint, *base_mint, YaError::BaseMintMismatch);
    Ok(())
}
```

### 4. Declare and validate the venue tail

Append the venue's accounts after prefix index 8 in `remaining_accounts`, in a fixed order you define. Validate the tail: check count, and each account's owner, key, type, and order. Return `YaError::InvalidRemainingAccounts` on any mismatch. Document the order in a header comment so integrators can build the call.

### 5. Write the CPI

```rust
ya_interface::CpiCall::global(venue_program_id, "<venue_instruction_name>")
    .arg(&amount)
    .account(account_a, /*is_signer*/ false, /*is_writable*/ true)
    .account(vault_authority, true, false)
    // ... the venue's account list, in its order ...
    .invoke_signed(
        account_infos,
        &[&[seeds::VAULT_AUTHORITY, position.key().as_ref(), &[vault_authority_bump]]],
    )?;
```

- Discriminator is `anchor_discriminator("global", "<name>")`. Confirm it against the IDL. A wrong one surfaces as `InstructionFallbackNotFound`.
- `vault_authority` is the only signer your program controls; it signs via `invoke_signed`.
- If the venue requires a refresh/crank (e.g. Kamino `refresh_reserve`), emit it as a separate top-level instruction (test `preInstructions`), not nested in this CPI.

### 6. current_value, the careful part

- Value the position: `shares × venue exchange rate`. Never a pool or reserve aggregate.
- Fail closed: stale or missing price source returns `YaError::OracleStale`.
- If the venue is a swap (you hold a wrapped token), execute the swap at the pool price with a `min_amount_out` guard, but compute `current_value` from an independent oracle exchange-rate feed, so the reported value cannot be moved by a single swap into the same pool. Execute against the market, price off the oracle.

### 7. Test against the fork

Wire the adapter into the parametrized suite. Inside a `describe`, register and approve the adapter, then:

```ts
runConformance(() => ({
  label: "<venue>",
  adapter: <venue>Program,
  baseMint: USDC_MINT,
  depositAmount: new BN(20_000_000),
  toleranceBps: 5,
  isInstant: true,                 // false + cooldownSeconds for two-phase venues
  initPosition: async () => { /* idempotent */ },
  depositRemaining: () => [ /* venue tail */ ],
  withdrawRemaining: () => [ /* tail incl. the withdrawal ticket */ ],
  valueRemaining: () => [ /* tail needed to price */ ],
  preInstructions: async () => [ /* refresh/crank if any */ ],
  vaultTokenAccount: () => vaultUsdcPda,
  ownerTokenAccount: () => ownerUsdcAta,
}));
```

Run it:

```bash
bash scripts/fork-test.sh tests/adapters/<venue>.spec.ts
```

The suite must pass all checks: init idempotent; deposit then `current_value` within tolerance; deposit-withdraw round-trips funds; impossible `min_*` reverts `SlippageExceeded`; a `Paused`/`Proposed` adapter reverts `AdapterNotActive`; a wrong base mint reverts `BaseMintMismatch`; a cooldown venue opens a `Pending` ticket and settles only after `unlock_ts`.

### 8. Register

Propose the adapter (program id + base mint), then approve it to `Active` via governance. The dispatcher routes to it with no redeploy. Confirm with `npm run verify:devnet`.

## Failure table

| Symptom | Cause | Fix |
|---|---|---|
| `InstructionFallbackNotFound` (101) | wrong discriminator, or the venue does not export that instruction by CPI | cross-check against the IDL; confirm the instruction is callable on the deployed program |
| Kamino deposit reverts only via CPI | old combined KLend instructions | use KLend V2 mutation instructions |
| oracle/reserve stale, only on the fork | cloned oracle `publish_time` older than the advancing fork clock | bump it with `surfnet_setAccount` in the test; list it in `tests/fork/FIXTURES.md`; production fails closed |
| Jupiter `add_liquidity2` tx too large | ~24 accounts | use an Address Lookup Table |
| `current_value` wildly wrong | reading a venue aggregate, not the position | `shares × exchange rate` |
| value diff a few lamports off | lazy interest accrued between read and redeem | pin both to the same instant for a diff=0 test, or assert within tolerance |
| swap CPI exceeds depth | routing through an aggregator | swap on one direct pool |

## Done

The adapter is complete when every conformance check is green against real cloned mainnet state and the adapter is `Active` in the registry. If a venue path is genuinely not executable on the deployed program, do not fake a pass: ship the spec-correct adapter, prove the blocker (e.g. a probe showing the instruction is not dispatched), and exercise the lifecycle against a clearly labeled stand-in. See `docs/adapters/drift-if.md` for the worked example of this case.
