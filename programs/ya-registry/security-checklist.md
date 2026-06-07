# Security checklist — ya-registry

**Risk level: 🟡 Medium** — holds admin keys (governance/guardian); custodies **no funds**; no CPI; multi-user reads.

Derived from the `safe-solana-builder` (Frank Castle) rules. Applied:

## Access control & identity (shared-base §1, §24)
- [x] Every gated action checks a signer: `Signer<'info>` + `has_one = governance` (propose/approve/resume/deprecate/propose_governance/set_guardian).
- [x] `pause_adapter` accepts guardian **or** governance via an explicit `require!(authority == governance || authority == guardian)` check (no implicit trust of account presence).
- [x] **Two-step governance rotation** (§24.2): `propose_governance` (current gov sets `pending_governance`) → `accept_governance` (only the proposed key, via `require_keys_eq!`, can accept). No single-call admin handover.
- [x] Guardian is a subordinate pause-only key, set directly by governance (documented decision — guardian cannot escalate; it can only pause).

## State & PDA safety (shared-base §1.5, §2)
- [x] `initialize_registry` uses `init` → callable exactly once (reinit attack blocked; test `reinitialize_fails`).
- [x] Canonical bumps stored (`registry.bump`, `adapter_entry.bump`) and reused in `seeds`/`bump =` constraints — never re-derived/brute-forced.
- [x] `AdapterEntry` PDA keyed by the adapter program id (`[b"adapter", program_id]`) → unique per program, no seed collision/sharing.
- [x] Status transitions validated (`approve` requires `Proposed`, `resume` requires `Paused`, `deprecate` rejects already-`Deprecated`) — test `approve_requires_proposed_status`.

## Arithmetic & input validation (shared-base §3, §18, §19)
- [x] `adapter_count` uses `checked_add` (`MathOverflow`).
- [x] Inputs validated at entry: `name.len() <= 32` (`NameTooLong`), `risk_tier <= 3` (`InvalidRiskTier`).
- [x] Descriptive `#[error_code]` errors; `require!`/`require_keys_eq!` over manual if/return.

## Events (shared-base §20)
- [x] Fixed-size structured events for every state change (RegistryInitialized, AdapterProposed, AdapterStatusChanged, GovernanceProposed/Accepted, GuardianSet); critical state persisted on-chain, not only in logs.

## High-risk decisions / known limitations
- Governance/guardian are plain pubkeys in the reference build (single keys). **For production: set governance to a Squads multisig** (zero code change) and consider a timelock around `accept_governance`. Flagged per §24.2.
- `propose_adapter` is restricted to governance (clean governance story). A permissionless-propose variant is possible (anyone proposes; governance approves) and documented in SPEC; not enabled here.
- The program is upgradeable (BPFLoaderUpgradeable); the upgrade authority should be the same multisig as governance in production.

## Tests (LiteSVM, `tests/registry_tests.rs`)
- [x] Happy-path full lifecycle with state assertions.
- [x] Wrong signer → fails (propose by non-governance; pause by random).
- [x] Re-initialization → fails.
- [x] Invalid status transition → fails.
- [x] Two-step rotation: impostor cannot accept; old governance loses authority after handover.
