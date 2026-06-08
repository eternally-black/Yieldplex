// M6/M7 acceptance: full fork-e2e for EVERY live adapter driven through the SDK's YieldAdapterClient,
// routed through the REAL on-chain dispatcher (not the adapter directly). Instant adapters
// (kamino/marginfi/jlp/maple): init -> deposit -> currentValue -> single-decoder read -> withdraw
// (instant settle). Two-phase (drift-if cooldown stand-in): init -> deposit -> withdraw (Pending) ->
// time-travel -> settle. Run standalone (own surfnet, no cross-spec position collisions):
//   bash scripts/fork-test.sh tests/sdk/e2e.spec.ts
import * as anchor from "@anchor-lang/core";
import { PublicKey, TransactionInstruction } from "@solana/web3.js";
import { assert } from "chai";
import * as fs from "fs";
import * as path from "path";
import {
  payer, provider, connection, dispatcher, registry, ensureRegistry, ensureActiveAdapter,
} from "../helpers/ctx";
import { fundUsdc, cheat, warpForwardSeconds } from "../helpers/cheatcodes";
import {
  YieldAdapterClient, AdapterDef, usdc,
  kaminoAdapter, marginfiAdapter, jlpAdapter, mapleAdapter,
  driftIfStandinAdapter, DRIFT_IF_COOLDOWN_SECONDS,
} from "../../ts/sdk";

const USDC = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
const idlDir = path.join(__dirname, "..", "..", "target", "idl");
const program = (idl: string) =>
  new anchor.Program(JSON.parse(fs.readFileSync(path.join(idlDir, `${idl}.json`), "utf8")), provider);

// Fork-only fixture: refresh the JLP doves_ag oracles' publish_time (i64 @177) — add/remove_liquidity2
// reject stale oracles (5s window). On mainnet the oracles are live, so this is a fork artifact only.
const JLP_DOVES_AG = [
  "FYq2BWQ1V5P1WFBqr3qB2Kb5yHVvSv7upzKodgQE5zXh", "AFZnHPzy4mvVCffrVwhewHbFc93uTHvDSFrVH7GtfXF1",
  "hUqAT1KQ7eW1i6Csp9CXYtpPfSAvi835V7wKi5fRfmC", "6Jp2xZUTWdDD2ZyUPRzeMdc6AFQ5K3pFgZxk2EijfjnM",
  "Fgc93D641F8N2d1xLjQ4jmShuD3GE3BsCXA56KBQbF5u",
].map((s) => new PublicKey(s));
async function freshenJlpOracles(): Promise<void> {
  const now = BigInt((await connection.getBlockTime(await connection.getSlot())) ?? 0);
  for (const o of JLP_DOVES_AG) {
    const ai = await connection.getAccountInfo(o);
    if (!ai) continue;
    const data = Buffer.from(ai.data);
    data.writeBigInt64LE(now, 177);
    await cheat("surfnet_setAccount", [o.toBase58(), {
      lamports: ai.lamports, data: data.toString("hex"), owner: ai.owner.toBase58(), executable: ai.executable,
    }]);
  }
}

interface InstantCase {
  label: string;
  program: anchor.Program;
  def: AdapterDef;
  toleranceLamports: number;
  fixture: () => Promise<void>;
}

const noop = async () => {};
const instantCases: InstantCase[] = [
  { label: "kamino", program: program("ya_adapter_kamino"), def: kaminoAdapter, toleranceLamports: 50_000, fixture: noop },
  { label: "marginfi", program: program("ya_adapter_marginfi"), def: marginfiAdapter, toleranceLamports: 50_000, fixture: noop },
  { label: "maple", program: program("ya_adapter_maple"), def: mapleAdapter, toleranceLamports: 250_000, fixture: noop },
  { label: "jlp", program: program("ya_adapter_jupiter_jlp"), def: jlpAdapter, toleranceLamports: 300_000, fixture: freshenJlpOracles },
];

describe("SDK e2e — every live adapter through YieldAdapterClient + the real dispatcher (surfnet)", () => {
  before(async () => {
    await ensureRegistry();
    await fundUsdc(payer.publicKey, 5_000_000_000n);
  });

  for (const c of instantCases) {
    it(`${c.label}: init -> deposit -> currentValue -> decode -> withdraw (instant)`, async () => {
      const client = new YieldAdapterClient({
        provider, dispatcher, adapter: c.program, def: c.def, registryProgramId: registry.programId,
      });
      await ensureActiveAdapter(c.program.programId, USDC, c.label);

      await client.initPosition();
      await c.fixture();
      await client.deposit(usdc(25));

      const value = await client.currentValue();
      assert.isNotNull(value, `${c.label}: currentValue must return data`);
      assert.isTrue(
        Math.abs(Number(value) - 25_000_000) <= c.toleranceLamports,
        `${c.label}: currentValue ${value} ~ 25 USDC (tol ${c.toleranceLamports})`,
      );

      const pos = await client.fetchPosition();
      assert.isNotNull(pos, `${c.label}: position must exist`);
      assert.equal(pos!.owner.toBase58(), payer.publicKey.toBase58(), `${c.label}: decoder owner`);
      assert.equal(pos!.adapter.toBase58(), c.program.programId.toBase58(), `${c.label}: decoder adapter id`);
      assert.isTrue(pos!.shares > 0n, `${c.label}: shares > 0`);

      await c.fixture();
      await client.withdraw(pos!.shares);
      const ticket = await client.fetchTicket();
      assert.isNotNull(ticket, `${c.label}: ticket must exist`);
      assert.equal(ticket!.status, "settled", `${c.label}: instant withdraw must settle`);
      console.log(`    [sdk-e2e/${c.label}] ok: currentValue=${value}, shares=${pos!.shares}`);
    });
  }

  it("drift-standin: init -> deposit -> withdraw(Pending) -> time-travel -> settle (two-phase)", async () => {
    const standin = program("ya_cooldown_standin");
    const client = new YieldAdapterClient({
      provider, dispatcher, adapter: standin, def: driftIfStandinAdapter, registryProgramId: registry.programId,
    });
    await ensureActiveAdapter(standin.programId, USDC, "drift-standin");

    await client.initPosition();
    await client.deposit(usdc(25));
    const pos = await client.fetchPosition();
    assert.isNotNull(pos, "standin: position must exist");
    assert.isTrue(pos!.shares > 0n, "standin: shares > 0 after deposit");

    await client.withdraw(pos!.shares);
    const pending = await client.fetchTicket();
    assert.isNotNull(pending, "standin: ticket must exist");
    assert.equal(pending!.status, "pending", "standin: two-phase withdraw must open Pending");
    assert.isTrue(pending!.unlockTs > 0n, "standin: Pending ticket must carry a future unlock");

    await warpForwardSeconds(connection, DRIFT_IF_COOLDOWN_SECONDS + 60);
    await client.settle();
    const settled = await client.fetchTicket();
    assert.isNotNull(settled, "standin: ticket must exist after settle");
    assert.equal(settled!.status, "settled", "standin: settle after cooldown must complete");
    console.log(`    [sdk-e2e/drift-standin] two-phase ok: Pending -> +${DRIFT_IF_COOLDOWN_SECONDS}s -> Settled`);
  });
});
