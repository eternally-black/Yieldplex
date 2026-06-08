// Drift Insurance Fund — two-phase (request -> cooldown -> settle).
//
// HONESTY: a live Drift IF-staking CPI is impossible for any integration today — the
// *_insurance_fund_stake instructions are commented out of Drift's deployed #[program]
// (see `yarn probe:drift-if`). The spec-correct two-phase adapter (ya-adapter-drift-if) is built
// and unit-tested but is NOT fork-runnable. What IS exercised end-to-end is the standard's two-phase
// machinery, via the LABELLED cooldown stand-in (ya-cooldown-standin). This def therefore points at
// the stand-in and must NEVER be presented as a live Drift pass. See docs/adapters/drift-if.md.
import * as anchor from "@anchor-lang/core";
import { PublicKey } from "@solana/web3.js";
import { AdapterDef, PositionContext, meta } from "./types";
import { USDC_MINT, SYSTEM_PROGRAM_ID } from "../constants";

export const COOLDOWN_STANDIN_PROGRAM_ID = new PublicKey("7aTuXKiyKwZ1MVrPfMgoAPoh8VKDSBLPAfEBvzkhJCYR");
/** Mirrors Drift's IF unstaking period (13 days). Read from chain in the real adapter. */
export const DRIFT_IF_COOLDOWN_SECONDS = 1_123_200;

/** The two-phase lifecycle stand-in. Labelled — proves request/Pending/settle, not a live Drift CPI. */
export const driftIfStandinAdapter: AdapterDef = {
  label: "drift-standin",
  programId: COOLDOWN_STANDIN_PROGRAM_ID,
  baseMint: USDC_MINT,
  isInstant: false,
  cooldownSeconds: DRIFT_IF_COOLDOWN_SECONDS,
  depositRemaining: () => [],
  valueRemaining: () => [],
  withdrawRemaining: (ctx) => [meta(ctx.ticket, true)],
  async buildInitPosition(program, ctx) {
    return program.methods
      .initializePosition()
      .accountsPartial({
        position: ctx.position,
        vaultAuthority: ctx.vaultAuthority,
        baseMint: ctx.baseMint,
        owner: ctx.owner,
        systemProgram: SYSTEM_PROGRAM_ID,
      })
      .instruction();
  },
};
