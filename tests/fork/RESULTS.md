# Mainnet-fork results (honest)

All adapters are tested on **Surfpool against real cloned mainnet state** (not LiteSVM, not a
localnet mock). Reproduce with `bash scripts/fork-test.sh <spec>`; the committed `fork-run.log` is a
real run. Toolchain: anchor `1.0.2` / solana `3.1.10` / surfpool `1.3.1`.

## Summary

| Adapter | Protocol | Live round-trip on cloned mainnet state | `current_value` accuracy | Tests |
|---|---|---|---|---|
| **kamino** | Kamino Lend (KLend) | ✅ deposit → value → withdraw | **diff = 0** vs the actual on-chain redemption | 10/10 |
| **marginfi** | MarginFi v2 | ✅ deposit → value → withdraw | **diff = 0** vs the actual on-chain redemption | 9/9 |
| **jupiter-jlp** | Jupiter Perps JLP | ✅ add → value → remove liquidity | **diff = 0** vs the pool NAV (`jlp × aum_usd / supply`) | 9/9 |
| **maple** | syrupUSDC via Orca Whirlpool | ✅ swap-and-hold round-trip | **diff = 0** vs the Chainlink exchange-rate value | 9/9 |
| **drift-if** | Drift v2 Insurance Fund | ⚠️ **live CPI disabled upstream** — see below | IF-share math (unit-tested); two-phase lifecycle ✅ on a stand-in | 10/10 (stand-in) + unit |

The 4 live adapters do a full `deposit → current_value → withdraw` against real cloned protocol
state, and each asserts `current_value` equals the protocol's own redemption/NAV math to the
lamport (the EDGE check) — stronger than the field's "value ≈ deposit". The mock adapter (8/8) is
the dispatcher/conformance reference, not a protocol.

### SDK (M6) + dispatcher fork-e2e (M7)

| Suite | What it proves | Tests |
|---|---|---|
| **conformance decoder gate** | the ONE SDK decoder (`decodePosition`) reads every adapter's on-chain `Position` and agrees with its Anchor-typed deserialization — runs inside the conformance suite for all 5 adapters + mock + stand-in | +1 per adapter (incl. above) |
| **`tests/sdk/decode.spec.ts`** | offline byte-vector round-trip of `Position` / `WithdrawalTicket` / `AdapterEntry` (+ discriminator rejection) | 4/4 |
| **`tests/sdk/e2e.spec.ts`** | every live adapter driven through `YieldAdapterClient` + the **real dispatcher** on fork: kamino/marginfi/jlp/maple init→deposit→`currentValue`→decode→withdraw (instant); drift stand-in init→deposit→withdraw(Pending)→time-travel→settle (two-phase) | 5/5 |

Counts: conformance fork suite + offline decoder = **59/59** (one surfnet); SDK e2e = **5/5** (own
surfnet, no cross-spec position collisions). The SDK is the same `@anchor-lang/core` route the
conformance harness uses, so the e2e proves the integrator surface, not a second code path.

## Drift Insurance Fund — the honest position (NOT a live pass)

A live Drift IF-staking CPI is **impossible for any integration today**: the IF-staking instructions
are commented out of Drift's deployed `#[program]` (`drift-labs/protocol-v2 programs/drift/src/lib.rs`
~796–880). We therefore ship and prove:
1. a **spec-correct** two-phase adapter (`ya-adapter-drift-if`) that runs the instant Drift re-enables
   the exports (real layout/discriminators, cooldown read from chain, IF-share value math — unit-tested);
2. `yarn probe:drift-if` — the live program rejects all IF-staking discriminators (corroborating;
   the source comment-out is the authoritative proof);
3. a **green two-phase lifecycle** (`deposit → request(Pending) → settle-too-early reverts →
   time-travel +cooldown → settle(Settled)`) against the labelled `ya-cooldown-standin` — proving
   our `request → cooldown → settle` machinery + the dispatcher's two-phase routing.

This is **never** presented as a live Drift pass. The validator/tooling version is not the cause — it
is an upstream `#[program]` export, removed at the source.

## Reproduce

- `bash scripts/fork-test.sh tests/adapters/kamino.spec.ts` (and marginfi / jlp / maple) — live round-trips + diff=0.
- `bash scripts/fork-test.sh tests/adapters/drift-if.spec.ts` — two-phase lifecycle on the stand-in.
- `bash scripts/fork-test.sh tests/sdk/e2e.spec.ts` — every adapter through the SDK + real dispatcher (M7).
- `npx ts-mocha -p ./tsconfig.json tests/sdk/decode.spec.ts` — offline single-decoder byte-vectors (M6).
- `yarn probe:drift-if` — Drift live-rejection + source citation.
- `bash scripts/test-rust.sh <crate>` — per-adapter value-math unit tests.

Fork-only fixtures (oracle/clock patches; no production path depends on them) are listed in
[`FIXTURES.md`](FIXTURES.md).
