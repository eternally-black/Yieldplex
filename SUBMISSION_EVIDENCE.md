# SUBMISSION EVIDENCE — Solana Yield Adapter Standard

> One-screen map from every requirement and judging criterion to a file path and a **runnable** verification command.
> Everything here is meant to be reproduced by the reviewer (human or agent). Claims that cannot be reproduced are not made.
>
> **Status legend:** ✅ done & reproducible now · 🚧 in progress (milestone) · placeholders `<…>` are filled from real runs as milestones land. No ✅ is asserted before it is produced.

**Build progress:** M0 toolchain/verify · M1 `ya-interface` · M2 `ya-registry` (6 tests) · M3 `ya-dispatcher` + mock + e2e (3 tests) · M4 conformance harness + **surfnet mainnet-fork pipeline** (parametrized `runConformance`, mock green on surfnet, TS client `@anchor-lang/core` 1.0.2, `tsc` clean) — **done**. M5 **five adapters + mainnet-fork tests — done** (4 adapters live with `current_value` diff=0 + Drift §F two-phase on a stand-in). M6 **TypeScript SDK (`ts/sdk`) — done**: `YieldAdapterClient` + the ONE `Position`/`WithdrawalTicket` decoder (validated against all 5 adapters via a conformance gate) + per-adapter account-builders + re-exported `runConformance`; `tsc` clean. M7 **dispatcher fork-e2e — done**: every live adapter driven through the SDK + the real dispatcher on fork (`tests/sdk/e2e.spec.ts`). **Fork totals: 59/59 conformance + 5/5 SDK e2e.** M8 devnet · M9 docs/skill/CI — **in progress**.

---

## 0. 60-second reviewer quickstart (the one thing to run)

```bash
git clone https://github.com/eternally-black/claude-lookup && cd claude-lookup
yarn install
export MAINNET_RPC_URL=<your-mainnet-rpc>     # required: the fork pulls live protocol state
bash scripts/fork-test.sh                      # Surfpool mainnet-fork conformance suite; expect 59 passing  ✅ M5/M6
bash scripts/fork-test.sh tests/sdk/e2e.spec.ts  # SDK + dispatcher e2e (own surfnet); expect 5 passing      ✅ M7
```

Pre-committed proof of the same runs (no setup needed to read):
- Results summary: [`tests/fork/RESULTS.md`](tests/fork/RESULTS.md) — ✅ M5/M6/M7
- Full log: [`tests/fork/fork-run.log`](tests/fork/fork-run.log) — ✅ real run (59/59 conformance + 5/5 SDK e2e)
- TS SDK: [`ts/sdk/README.md`](ts/sdk/README.md) — ✅ M6 (`YieldAdapterClient` + the single decoder)
- CI (runs the suite on every push): `.github/workflows/ci.yml` → badge in [`README.md`](README.md) — 🚧 M9

Available **now** (M0–M3) without an RPC:
```bash
bash scripts/build.sh                          # anchor build: ya-registry + ya-dispatcher + ya-mock-adapter (+IDLs) — green
bash scripts/test-rust.sh ya-registry          # 6 LiteSVM tests: lifecycle + auth + two-step governance rotation
bash scripts/test-rust.sh ya-dispatcher        # 3 LiteSVM e2e: registry->dispatcher->adapter route + returnData view + gating
node scripts/verify-addresses.mjs              # re-verify all §8 mainnet addresses on-chain (needs MAINNET_RPC_URL)
```

---

## 1. Submission requirements → evidence → command

| Requirement | Where it lives | Verify it yourself | Status |
|---|---|---|---|
| Public GitHub repo with all source | `https://github.com/eternally-black/claude-lookup` | `git remote -v` | ✅ (private during build; public at submission) |
| Core dispatcher (router: `deposit`, `withdraw`, `current_value`) | `programs/ya-dispatcher/src/lib.rs` | `bash scripts/test-rust.sh ya-dispatcher` | ✅ M3 |
| Five reference adapters | `programs/ya-adapter-{kamino,marginfi,jupiter-jlp,maple,drift-if}/` | `anchor build` (all members) | ✅ M5 (all 5 built; 4 live fork + Drift §F) |
| Governance-gated on-chain registry | `programs/ya-registry/src/lib.rs` (`propose` → governance `approve` → `pause`/`deprecate`) | `bash scripts/test-rust.sh ya-registry` | ✅ M2 |
| Mainnet-fork tests, all five adapters | `tests/adapters/*.spec.ts` + conformance suite | `bash scripts/fork-test.sh` (see §0) | ✅ M5 — 59/59 green (4/5 live-protocol; Drift = §4) |
| TypeScript SDK (client + single decoder) | `ts/sdk/` (`YieldAdapterClient`, `decodePosition`, account-builders) | `bash scripts/fork-test.sh tests/sdk/e2e.spec.ts` | ✅ M6/M7 — 5/5 e2e through the dispatcher |
| Registry deployed to devnet | `deploy/devnet.json` + `README.md` addresses | `yarn verify:devnet` | 🚧 M8 |
| Adapter standard spec (markdown) | [`docs/SPEC.md`](docs/SPEC.md) | open it | 🚧 M9 |
| "Build your own adapter" guide | [`docs/BUILD_YOUR_OWN_ADAPTER.md`](docs/BUILD_YOUR_OWN_ADAPTER.md) | open it | 🚧 M9 |
| (bonus) Agent-native skill | [`skills/build-yield-adapter/SKILL.md`](skills/build-yield-adapter/SKILL.md) | drop into any SKILL.md-compatible agent | 🚧 M9 |

---

## 2. Judging criteria → where we are strongest

| Criterion (weight) | Evidence | One-line claim a reviewer can verify |
|---|---|---|
| **Correctness (40%)** | `tests/fork/RESULTS.md`, `fork-run.log`, conformance suite | **59 fork tests green + 5 SDK e2e**; 4/5 adapters do a full `deposit → current_value → withdraw` round-trip against **real cloned mainnet state** with `current_value` matching the protocol's own redemption/NAV math to **0 lamports**; Drift two-phase lifecycle proven on a stand-in (§4); the SDK drives the same flow through the real dispatcher for every adapter. — ✅ M5–M7 |
| **Interface design (25%)** | `crates/ya-interface/` + `ts/sdk/` | One uniform adapter shape; standard account prefix (9) + opaque `remaining_accounts`; `current_value` as a real view via `set_return_data`; **two-phase withdrawal (request → settle)** that handles instant *and* cooldown adapters; adding an adapter touches **zero** lines of dispatcher/registry; integrator SDK = one `YieldAdapterClient` + **one decoder for all adapters**. — ✅ M1–M3, M6 |
| **Developer guide (20%)** | `ts/sdk/README.md` + `docs/BUILD_YOUR_OWN_ADAPTER.md` + `skills/build-yield-adapter/` | SDK quickstart (swap one import to change protocol) — ✅ M6; a worked "Hello Yield" example from empty folder to green conformance + an **agent skill** — 🚧 M9. |
| **Code quality + tests (15%)** | `crates/ya-interface`, `ts/sdk`, conformance suite, CI | One uniform manual-CPI primitive (`ya_interface::CpiCall`); `tsc` clean; the single decoder validated against all 5 adapters by a conformance gate; conformance suite runs the same checks against every adapter; CI gates merges — 🚧 M9. — ✅ (primitive M1; SDK + decoder gate M6) |

---

## 3. Adapter status (machine-readable, honest)

Each "live" row = full standard flow against **real cloned mainnet state**: `init_registry → whitelist → open_position → deposit → current_value → withdraw`. **59/59 conformance fork tests + 5/5 SDK e2e green** — see [`tests/fork/RESULTS.md`](tests/fork/RESULTS.md) + [`tests/fork/fork-run.log`](tests/fork/fork-run.log). (Each adapter's count below includes the M6 *single-decoder* gate.)

| Adapter | Protocol CPI | Build | Live-protocol round-trip | `current_value` accuracy | Edge cases | Status |
|---|---|---|---|---|---|---|
| `kamino` | Kamino Lend (KLend) | ✅ | ✅ deposit/value/withdraw (10/10) | **diff vs actual on-chain redemption = `0`** | impossible-min revert; paused + base-mint gating | ✅ M5 |
| `marginfi` | MarginFi v2 bank | ✅ | ✅ deposit/value/withdraw (9/9) | **diff vs actual on-chain redemption = `0`** | impossible-min revert; paused + base-mint gating | ✅ M5 |
| `jupiter-jlp` | Jupiter Perps JLP (add/remove liq, 24 accts) | ✅ | ✅ add/remove-liquidity (9/9) | **diff vs pool NAV (jlp×aum/supply) = `0`** | impossible-min revert; paused + base-mint gating | ✅ M5 |
| `maple` | syrupUSDC via Orca Whirlpool (swap-and-hold) | ✅ | ✅ swap-and-hold round-trip (9/9) | **diff vs Chainlink exchange-rate value = `0`** (not the pool quote) | impossible-min revert; paused + base-mint gating | ✅ M5 |
| `drift-if` | Drift v2 Insurance Fund | ✅ | ⚠️ see §4 — **live CPI disabled upstream** | IF-share math (unit-tested); two-phase lifecycle ✅ on stand-in (10/10) | request→cooldown→settle + too-early-settle revert | ✅ M5 (adapter + lifecycle; ⚠️ live CPI blocked upstream) |
| **SDK** | `YieldAdapterClient` + single decoder | ✅ | ✅ all 5 through the **real dispatcher** (`tests/sdk/e2e.spec.ts`, 5/5) | one decoder reads every adapter's `Position` | offline byte-vectors (`tests/sdk/decode.spec.ts`, 4/4) | ✅ M6/M7 |

> Value claim, proven on the fork: each live adapter's `current_value` is diffed against the protocol's **own redemption/NAV math** (not a possibly-divergent SDK) to **`0` lamports** — the EDGE test in each `tests/adapters/<name>.spec.ts`, captured in `fork-run.log`.

---

## 4. Drift Insurance Fund — the honest, evidence-backed position

A real Drift IF-staking CPI round-trip is **not achievable against the currently deployed Drift program**, for any submission, because the IF-staking instructions are **commented out** in Drift's deployed program. This is verifiable, not an opinion:

- **Source proof:** `drift-labs/protocol-v2` `programs/drift/src/lib.rs` — `initialize_/add_/request_remove_/cancel_request_remove_/remove_insurance_fund_stake` are all commented out of the `#[program]` block (see `docs/adapters/drift-if.md` for the exact lines). — 🚧 M5
- **On-chain proof:** `yarn probe:drift-if` sends each candidate discriminator (verified against `@drift-labs/sdk`) to the live program and shows it returns `InstructionFallbackNotFound (101)` before account validation. — 🚧 M5

What we therefore ship, and prove:

1. A **spec-correct Drift IF adapter** with the real account layout and discriminators from `@drift-labs/sdk` — it will execute the instant Drift re-enables the exports. Not a stub.
2. A **green two-phase lifecycle test** (`deposit → request_withdraw → time-travel +cooldown → settle`) run against a minimal conformance stand-in that exposes the same IF instruction surface — proving **our** adapter logic and the dispatcher's two-phase routing are correct and reproducible.
3. The two proofs above, so a reviewer can confirm both the upstream block and our correctness in one command each.

We do **not** label the stand-in run as a live-protocol pass. The distinction is explicit in `RESULTS.md`.

---

## 5. Reproducibility & honest limitations (read this — it is part of the evidence)

**Reproducibility**
- `bash scripts/fork-test.sh` runs the conformance suite on Surfpool (mainnet-fork); `tests/sdk/e2e.spec.ts` runs the SDK e2e in its own surfnet. `MAINNET_RPC_URL` is the only external input.
- `tests/fork/RESULTS.md` + `fork-run.log` are committed from a real run (Anchor 1.0.2 / Solana 3.1.10 / Surfpool 1.3.1): 59/59 conformance + 5/5 SDK e2e. — ✅ M5–M7
- CI re-runs the suite on every push. — 🚧 M9

**Limitations, stated plainly (so nothing surprises a reviewer):**
- **Drift IF:** live CPI disabled upstream (§4). Adapter is spec-correct; lifecycle proven on a stand-in.
- **Maple:** syrupUSDC has no native synchronous Solana deposit (it is a Chainlink CCIP cross-chain token; the lending lives on Ethereum). The adapter acquires/exits syrupUSDC via a single direct Orca Whirlpool pool — the correct synchronous Solana primitive. Entry/exit is therefore liquidity-constrained (real slippage, enforced via `min_amount_out`).
- **Fork-only fixtures:** where a cloned oracle/reserve is stale on the fork, the test patches it to a fresh value; every such patch is listed in `tests/fork/FIXTURES.md`. No production code path depends on these.

**Toolchain note:** built on Anchor `1.0.2` / Solana `3.1.10` via Surfpool. The pinned `0.31.1 / 2.2.20` stack is **explicitly waived by the sponsor** — bounty Q&A: *"Can we ignore the tech stack and use the latest versions of anchor-cli and Solana?"* → Sponsor (Serhii Kovalchuk): *"yes"* (bounty: https://superteam.fun/earn/listing/develop-solana-yield-adapter-standard). Using the latest toolchain is therefore fully conforming, not a deviation.

---

## 6. Verification command index (copy-paste)

```bash
# available now (M0–M3):
bash scripts/build.sh                          # anchor build all programs + IDLs (green)
bash scripts/test-rust.sh ya-registry          # registry lifecycle + governance auth rejection (6 tests)
bash scripts/test-rust.sh ya-dispatcher        # router e2e + returnData view + gating (3 tests)
node scripts/verify-addresses.mjs              # re-verify §8 mainnet addresses (needs MAINNET_RPC_URL)

# mainnet-fork (M5–M7, now):
bash scripts/fork-test.sh                       # conformance fork suite (Surfpool) — 59/59 (drift-if last)
bash scripts/fork-test.sh tests/sdk/e2e.spec.ts # SDK + real-dispatcher e2e, every adapter (own surfnet) — 5/5
bash scripts/fork-test.sh tests/adapters/kamino.spec.ts   # one adapter (kamino|marginfi|jlp|maple|drift-if)
npx ts-mocha -p ./tsconfig.json tests/sdk/decode.spec.ts  # offline single-decoder byte-vectors — 4/4
yarn probe:drift-if                            # Drift IF live-rejection + source citation (the §4 evidence)
bash scripts/test-rust.sh ya-adapter-kamino    # per-adapter value-math unit test (each crate)

# arriving M8–M9:
yarn verify:devnet                             # devnet registry: programs executable + N active adapters
```
