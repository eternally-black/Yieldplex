// The reusable, parametrized conformance suite (differentiator W3).
// Every adapter passes the SAME checks. An adapter author calls `runConformance(() => cfg)`
// inside a `describe(...)` (after registering + initializing their adapter) and gets an instant
// pass/fail against the Yield Adapter Standard. Protocol-specific accounts/refresh are injected
// via the builder callbacks, so the suite itself is adapter-agnostic.
import { assert } from "chai";
import * as anchor from "@anchor-lang/core";
import { AccountMeta, PublicKey, TransactionInstruction } from "@solana/web3.js";
import {
  dispatcher, registry, registryPda, entryPda, payer,
  positionPda, ticketPda, routeAccounts, readReturnedU64,
} from "../helpers/ctx";

export interface ConformanceConfig {
  label: string;
  adapter: anchor.Program;            // adapter program client (for account fetch)
  baseMint: PublicKey;
  depositAmount: anchor.BN;
  toleranceBps: number;               // value/withdraw rounding+fee tolerance
  isInstant: boolean;                 // instant (settle in withdraw) vs two-phase (request->settle)
  cooldownSeconds?: number;           // for two-phase adapters
  /** Adapter-specific accounts appended AFTER the standard 9-account prefix. */
  depositRemaining?: () => AccountMeta[];
  withdrawRemaining?: () => AccountMeta[];   // must include the withdrawal ticket
  valueRemaining?: () => AccountMeta[];
  /** Top-level instructions to prepend (e.g. Kamino refresh_reserve, Drift cranks). */
  preInstructions?: () => Promise<TransactionInstruction[]>;
  /** Create the Position (and protocol sub-accounts). Must be idempotent. */
  initPosition: () => Promise<void>;
}

const BN0 = new anchor.BN(0);
const tol = (amount: anchor.BN, bps: number) => amount.muln(bps).divn(10000);
const absDiff = (a: anchor.BN, b: anchor.BN) => (a.gt(b) ? a.sub(b) : b.sub(a));

export function runConformance(get: () => ConformanceConfig): void {
  const owner = payer.publicKey;
  const acc = (cfg: ConformanceConfig, baseMintOverride?: PublicKey) =>
    routeAccounts(cfg.adapter.programId, owner, cfg.baseMint, baseMintOverride);
  const pre = async (cfg: ConformanceConfig) => (cfg.preInstructions ? await cfg.preInstructions() : []);

  it("initialize_position is idempotent", async () => {
    const cfg = get();
    await cfg.initPosition();
    await cfg.initPosition(); // second call must not throw
  });

  it("deposit then current_value ≈ amount (within tolerance)", async () => {
    const cfg = get();
    await dispatcher.methods
      .routeDeposit(cfg.depositAmount, BN0)
      .accountsPartial(acc(cfg))
      .remainingAccounts(cfg.depositRemaining?.() ?? [])
      .preInstructions(await pre(cfg))
      .rpc();

    const tx = await dispatcher.methods
      .routeCurrentValue()
      .accountsPartial(acc(cfg))
      .remainingAccounts(cfg.valueRemaining?.() ?? cfg.depositRemaining?.() ?? [])
      .preInstructions(await pre(cfg))
      .transaction();
    const value = await readReturnedU64(tx);
    assert.isNotNull(value, "current_value must return data");
    const v = new anchor.BN(value!.toString());
    const t = tol(cfg.depositAmount, cfg.toleranceBps);
    assert.isTrue(
      absDiff(v, cfg.depositAmount).lte(t),
      `[${cfg.label}] value ${v.toString()} vs deposit ${cfg.depositAmount.toString()} (tol ${t.toString()})`,
    );
  });

  it("position reflects nonzero shares", async () => {
    const cfg = get();
    const pos: any = await (cfg.adapter.account as any).position.fetch(positionPda(cfg.adapter.programId, owner, cfg.baseMint));
    assert.isTrue(new anchor.BN(pos.shares.toString()).gtn(0), "shares should be > 0 after deposit");
  });

  it("impossible min_position_out reverts (SlippageExceeded)", async () => {
    const cfg = get();
    let failed = false;
    try {
      await dispatcher.methods
        .routeDeposit(new anchor.BN(1), new anchor.BN("18446744073709551615"))
        .accountsPartial(acc(cfg))
        .remainingAccounts(cfg.depositRemaining?.() ?? [])
        .preInstructions(await pre(cfg))
        .rpc();
    } catch {
      failed = true;
    }
    assert.isTrue(failed, "deposit with impossible min_position_out must revert");
  });

  it("withdraw settles instantly / opens a pending ticket (two-phase)", async () => {
    const cfg = get();
    const position = positionPda(cfg.adapter.programId, owner, cfg.baseMint);
    const pos: any = await (cfg.adapter.account as any).position.fetch(position);
    const shares = new anchor.BN(pos.shares.toString());

    await dispatcher.methods
      .routeWithdraw(shares, BN0)
      .accountsPartial(acc(cfg))
      .remainingAccounts(cfg.withdrawRemaining?.() ?? [])
      .preInstructions(await pre(cfg))
      .rpc();

    const ticket: any = await (cfg.adapter.account as any).withdrawalTicket.fetch(ticketPda(cfg.adapter.programId, position));
    if (cfg.isInstant) {
      assert.equal(Object.keys(ticket.status)[0], "settled", "instant withdraw must settle in one tx");
    } else {
      assert.equal(Object.keys(ticket.status)[0], "pending", "two-phase withdraw must open a pending ticket");
      assert.isTrue(new anchor.BN(ticket.unlockTs.toString()).gtn(0), "pending ticket must have a future unlock");
    }
  });

  it("registry gating: paused adapter routing reverts (AdapterNotActive)", async () => {
    const cfg = get();
    const entry = entryPda(cfg.adapter.programId);
    await registry.methods.pauseAdapter().accountsPartial({ registry: registryPda(), adapterEntry: entry, authority: payer.publicKey }).rpc();
    let failed = false;
    try {
      await dispatcher.methods.routeDeposit(new anchor.BN(1), BN0).accountsPartial(acc(cfg)).remainingAccounts(cfg.depositRemaining?.() ?? []).preInstructions(await pre(cfg)).rpc();
    } catch {
      failed = true;
    }
    await registry.methods.resumeAdapter().accountsPartial({ registry: registryPda(), adapterEntry: entry, governance: payer.publicKey }).rpc();
    assert.isTrue(failed, "routing a paused adapter must fail");
  });

  it("base-mint mismatch routing reverts (BaseMintMismatch)", async () => {
    const cfg = get();
    let failed = false;
    try {
      await dispatcher.methods.routeDeposit(new anchor.BN(1), BN0).accountsPartial(acc(cfg, PublicKey.unique())).remainingAccounts(cfg.depositRemaining?.() ?? []).preInstructions(await pre(cfg)).rpc();
    } catch {
      failed = true;
    }
    assert.isTrue(failed, "routing with a wrong base_mint must fail");
  });
}
