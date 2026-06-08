// Jupiter JLP adapter — mainnet-fork conformance + NAV value-accuracy.
// add/remove_liquidity2 revalue the whole pool, so the CPI carries 24 accounts (14 + all 5
// custodies + their 5 doves_ag oracles). The doves_ag oracles have a 5s staleness window, so we
// refresh them before each op (fork-only fixture). value = jlp_balance * aum_usd / JLP supply (NAV).
import * as anchor from "@anchor-lang/core";
import { AccountMeta, ComputeBudgetProgram, PublicKey } from "@solana/web3.js";
import { assert } from "chai";
import * as fs from "fs";
import * as path from "path";
import {
  payer, provider, connection, dispatcher, routeAccounts, readReturnedU64,
  positionPda, vaultAuthorityPda, ticketPda, ensureRegistry, ensureActiveAdapter,
  SYSTEM_PROGRAM, TOKEN_PROGRAM, ata,
} from "../helpers/ctx";
import { runConformance } from "../conformance/runConformance";
import { fundUsdc, cheat } from "../helpers/cheatcodes";

const USDC = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
const PERPS = new PublicKey("PERPHjGBqRHArX4DySjwM6UJHiR3sWAatqfdBS2qQJu");
const POOL = new PublicKey("5BUwFW4nRbftYTDMbgxykoFWqWHPzahFSNAaaaJtVKsq");
const JLP_MINT = new PublicKey("27G8MtK7VtTcCHkpASjSDdkWWYfoqT6ggEuKidVJidD4");
const PERPETUALS = new PublicKey("H4ND9aYttUVLFmNypZqLjZ52FYiGvdEB45GmwNoKEjTj");
const TRANSFER_AUTH = new PublicKey("AVzP2GeRmqGphJsMxWoqjpUifPpCret7LqWhD8NWQK49");
const EVENT_AUTH = new PublicKey("37hJBDnntwqhGbK7L6M1bLyvccj4u55CCUiLPdYkiqBN");
const USDC_CUSTODY = new PublicKey("G18jKKXQwBbrHeiK3C9MRXhkHsLHf7XgCSisykV46EZa");
const USDC_CUSTODY_TOKEN = new PublicKey("WzWUoCmtVv7eqAbU3BfKPU3fhLP6CXR8NCJH78UK9VS");
const USDC_DOVES_AG = new PublicKey("6Jp2xZUTWdDD2ZyUPRzeMdc6AFQ5K3pFgZxk2EijfjnM");
const CUSTODIES = [
  "7xS2gz2bTp3fwCC7knJvUWTEU9Tycczu6VhJYKgi1wdz", "AQCGyheWPLeo6Qp9WpYS9m3Qj479t7R636N9ey1rEjEn",
  "5Pv3gM9JrFFH883SWAhvJC9RPYmo8UNxuFtv5bMMALkm", "G18jKKXQwBbrHeiK3C9MRXhkHsLHf7XgCSisykV46EZa",
  "4vkNeXiYEUizLdrpdPS1eC2mccyM4NUPRtERrk6ZETkk",
].map((s) => new PublicKey(s));
const DOVES_AG = [
  "FYq2BWQ1V5P1WFBqr3qB2Kb5yHVvSv7upzKodgQE5zXh", "AFZnHPzy4mvVCffrVwhewHbFc93uTHvDSFrVH7GtfXF1",
  "hUqAT1KQ7eW1i6Csp9CXYtpPfSAvi835V7wKi5fRfmC", "6Jp2xZUTWdDD2ZyUPRzeMdc6AFQ5K3pFgZxk2EijfjnM",
  "Fgc93D641F8N2d1xLjQ4jmShuD3GE3BsCXA56KBQbF5u",
].map((s) => new PublicKey(s));

const idlDir = path.join(__dirname, "..", "..", "target", "idl");
const jlp = new anchor.Program(
  JSON.parse(fs.readFileSync(path.join(idlDir, "ya_adapter_jupiter_jlp.json"), "utf8")),
  provider,
);

// Refresh the doves_ag oracles' publish_time (i64 @177) to the current clock (5s staleness window).
async function freshenOracles() {
  const now = BigInt((await connection.getBlockTime(await connection.getSlot())) ?? 0);
  for (const o of DOVES_AG) {
    const ai = await connection.getAccountInfo(o);
    if (!ai) continue;
    const data = Buffer.from(ai.data);
    data.writeBigInt64LE(now, 177);
    await cheat("surfnet_setAccount", [o.toBase58(), {
      lamports: ai.lamports, data: data.toString("hex"), owner: ai.owner.toBase58(), executable: ai.executable,
    }]);
  }
}

function parseAum(buf: Buffer): bigint {
  const nameLen = buf.readUInt32LE(8);
  const cOff = 12 + nameLen;
  const nCust = buf.readUInt32LE(cOff);
  const aumOff = cOff + 4 + nCust * 32;
  let aum = 0n;
  for (let i = aumOff + 15; i >= aumOff; i--) aum = (aum << 8n) | BigInt(buf[i]);
  return aum;
}

describe("ya-adapter-jupiter-jlp — conformance + NAV accuracy (surfnet, real Jupiter state)", () => {
  const owner = payer.publicKey;
  const adapter = jlp.programId;
  const position = positionPda(adapter, owner, USDC);
  const vaultAuthority = vaultAuthorityPda(adapter, position);
  const vaultUsdc = PublicKey.findProgramAddressSync([Buffer.from("vault_usdc"), position.toBuffer()], adapter)[0];
  const vaultJlp = PublicKey.findProgramAddressSync([Buffer.from("vault_jlp"), position.toBuffer()], adapter)[0];
  const ownerUsdc = ata(USDC, owner);
  const m = (k: PublicKey, w: boolean): AccountMeta => ({ pubkey: k, isSigner: false, isWritable: w });
  // base 11 (vault_jlp..program) + trailing 10 (5 custodies + 5 doves_ag), per adapter R-indices.
  const protocolMetas = (): AccountMeta[] => [
    m(vaultJlp, true), m(TRANSFER_AUTH, false), m(PERPETUALS, false), m(POOL, true),
    m(USDC_CUSTODY, true), m(USDC_DOVES_AG, false), m(USDC_DOVES_AG, false), m(USDC_CUSTODY_TOKEN, true),
    m(JLP_MINT, true), m(EVENT_AUTH, false), m(PERPS, false),
    ...CUSTODIES.map((c) => m(c, false)), ...DOVES_AG.map((d) => m(d, false)),
  ];
  const cuPre = async () => { await freshenOracles(); return [ComputeBudgetProgram.setComputeUnitLimit({ units: 800_000 })]; };
  const acc = () => routeAccounts(adapter, owner, USDC, { vaultTokenAccount: vaultUsdc, ownerTokenAccount: ownerUsdc });

  before(async () => {
    await ensureRegistry();
    await ensureActiveAdapter(adapter, USDC, "jlp");
    await fundUsdc(owner, 1_000_000_000n);
  });

  runConformance(() => ({
    label: "jlp",
    adapter: jlp,
    baseMint: USDC,
    depositAmount: new anchor.BN(25_000_000),
    toleranceBps: 60, // add/remove_liquidity2 fee (~20bp) + dynamic tax
    isInstant: true,
    vaultTokenAccount: () => vaultUsdc,
    ownerTokenAccount: () => ownerUsdc,
    depositRemaining: protocolMetas,
    valueRemaining: () => [m(POOL, false), m(JLP_MINT, false)],
    withdrawRemaining: () => [m(ticketPda(adapter, position), true), ...protocolMetas()],
    preInstructions: cuPre,
    initPosition: async () => {
      await jlp.methods
        .initializePosition()
        .accountsPartial({
          position, vaultAuthority, baseMint: USDC, jlpMint: JLP_MINT, vaultUsdc, vaultJlp,
          owner, tokenProgram: TOKEN_PROGRAM, systemProgram: SYSTEM_PROGRAM,
        })
        .rpc();
    },
  }));

  it("EDGE: current_value() == pool NAV (jlp × aum_usd / supply), diff = 0", async () => {
    // Fresh deposit so the position holds JLP (conformance removed all liquidity).
    await dispatcher.methods
      .routeDeposit(new anchor.BN(20_000_000), new anchor.BN(0))
      .accountsPartial(acc())
      .remainingAccounts(protocolMetas())
      .preInstructions(await cuPre())
      .rpc();
    const tx = await dispatcher.methods
      .routeCurrentValue()
      .accountsPartial(acc())
      .remainingAccounts([m(POOL, false), m(JLP_MINT, false)])
      .preInstructions(await cuPre())
      .transaction();
    const value = await readReturnedU64(tx);
    assert.isNotNull(value, "current_value must return data");

    const jlpBal = BigInt((await connection.getTokenAccountBalance(vaultJlp)).value.amount);
    const aum = parseAum(Buffer.from((await connection.getAccountInfo(POOL))!.data));
    const supply = BigInt((await connection.getTokenSupply(JLP_MINT)).value.amount);
    const expected = (jlpBal * aum) / supply;
    console.log(`    [jlp] jlp=${jlpBal} aum=${aum} supply=${supply} current_value=${value} expected=${expected}`);
    assert.equal((value as bigint).toString(), expected.toString(), "current_value must equal NAV (diff=0)");
  });
});
