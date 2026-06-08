// Drift Insurance Fund — the honest Drift proof.
// A live Drift IF-staking CPI is impossible for any integration (the *_insurance_fund_stake
// instructions are commented out of Drift's deployed #[program]; see `yarn probe:drift-if` +
// docs/adapters/drift-if.md). The spec-correct two-phase adapter (ya-adapter-drift-if) is built and
// unit-tested. Here we prove OUR two-phase machinery — the standard request->cooldown->settle
// withdrawal + the dispatcher's two-phase routing — is correct, against the labelled cooldown
// STAND-IN (ya-cooldown-standin). This is NEVER a live Drift pass.
import * as anchor from "@anchor-lang/core";
import { AccountMeta, PublicKey } from "@solana/web3.js";
import { assert } from "chai";
import * as fs from "fs";
import * as path from "path";
import {
  payer, provider, connection, dispatcher, positionPda, vaultAuthorityPda, ticketPda,
  ensureRegistry, ensureActiveAdapter, routeAccounts, SYSTEM_PROGRAM,
} from "../helpers/ctx";
import { runConformance } from "../conformance/runConformance";
import { warpForwardSeconds } from "../helpers/cheatcodes";

const USDC = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
const COOLDOWN = 1_123_200; // 13 days, mirrors Drift's IF unstaking period

const idlDir = path.join(__dirname, "..", "..", "target", "idl");
const standin = new anchor.Program(
  JSON.parse(fs.readFileSync(path.join(idlDir, "ya_cooldown_standin.json"), "utf8")),
  provider,
);

describe("Drift IF two-phase lifecycle — via the cooldown STAND-IN (surfnet); NOT a live Drift pass", () => {
  const owner = payer.publicKey;
  const adapter = standin.programId;
  const position = positionPda(adapter, owner, USDC);
  const ticket = ticketPda(adapter, position);
  const acc = () => routeAccounts(adapter, owner, USDC);
  const ticketMeta = (): AccountMeta[] => [{ pubkey: ticket, isSigner: false, isWritable: true }];

  before(async () => {
    await ensureRegistry();
    await ensureActiveAdapter(adapter, USDC, "drift-standin");
  });

  // Standard conformance with isInstant=false: deposit, value, and the two-phase withdraw that must
  // open a Pending ticket with a future unlock (instead of settling in one call).
  runConformance(() => ({
    label: "drift-standin",
    adapter: standin,
    baseMint: USDC,
    depositAmount: new anchor.BN(25_000_000),
    toleranceBps: 0,
    isInstant: false,
    cooldownSeconds: COOLDOWN,
    withdrawRemaining: ticketMeta,
    initPosition: async () => {
      await standin.methods
        .initializePosition()
        .accountsPartial({ position, vaultAuthority: vaultAuthorityPda(adapter, position), baseMint: USDC, owner, systemProgram: SYSTEM_PROGRAM })
        .rpc();
    },
  }));

  it("settle before unlock reverts (WithdrawalLocked)", async () => {
    // The conformance two-phase withdraw above left a Pending ticket; settling now must fail.
    let failed = false;
    try {
      await dispatcher.methods.routeSettleWithdrawal(new anchor.BN(0)).accountsPartial(acc()).remainingAccounts(ticketMeta()).rpc();
    } catch { failed = true; }
    assert.isTrue(failed, "settle before cooldown unlock must revert");
  });

  it("settle after cooldown completes the two-phase withdrawal (Pending -> Settled)", async () => {
    const posBefore: any = await (standin.account as any).position.fetch(position);
    const t: any = await (standin.account as any).withdrawalTicket.fetch(ticket);
    assert.equal(Object.keys(t.status)[0], "pending", "ticket should be Pending from the conformance withdraw");

    await warpForwardSeconds(connection, COOLDOWN + 60);
    await dispatcher.methods.routeSettleWithdrawal(new anchor.BN(0)).accountsPartial(acc()).remainingAccounts(ticketMeta()).rpc();

    const tAfter: any = await (standin.account as any).withdrawalTicket.fetch(ticket);
    const posAfter: any = await (standin.account as any).position.fetch(position);
    assert.equal(Object.keys(tAfter.status)[0], "settled", "ticket must be Settled after cooldown settle");
    const reduced = new anchor.BN(posBefore.shares.toString()).sub(new anchor.BN(posAfter.shares.toString()));
    assert.equal(reduced.toString(), t.shares.toString(), "settle must reduce shares by the requested amount");
    console.log(`    [drift-if/standin] two-phase complete: requested ${t.shares} shares, Pending -> time-travel +${COOLDOWN}s -> Settled`);
  });
});
