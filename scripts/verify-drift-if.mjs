// Drift IF research verification: resolve PDAs for market 0 (USDC), read SpotMarket
// on-chain to confirm offsets + live values, and probe the live program with each IF
// discriminator to confirm InstructionFallbackNotFound (101).
import {
  Connection, PublicKey, TransactionInstruction, VersionedTransaction, TransactionMessage,
} from "@solana/web3.js";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.dirname(HERE);
// load .env
const env = fs.readFileSync(path.join(REPO, ".env"), "utf8");
const RPC = env.match(/MAINNET_RPC_URL=(.+)/)[1].trim();
const conn = new Connection(RPC, "confirmed");

const DRIFT = new PublicKey("dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH");
const USDC = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
const enc = (s) => Buffer.from(s);
const u16le = (n) => { const b = Buffer.alloc(2); b.writeUInt16LE(n); return b; };

const market = 0;
function pda(seeds) { return PublicKey.findProgramAddressSync(seeds, DRIFT); }

const state = pda([enc("drift_state")]);
const spotMarket = pda([enc("spot_market"), u16le(market)]);
const spotMarketVault = pda([enc("spot_market_vault"), u16le(market)]);
const insuranceFundVault = pda([enc("insurance_fund_vault"), u16le(market)]);
const driftSigner = pda([enc("drift_signer")]);

console.log("=== RESOLVED PDAs (market_index 0, USDC) ===");
console.log("drift state            ", state[0].toBase58(), "bump", state[1]);
console.log("spot_market            ", spotMarket[0].toBase58(), "bump", spotMarket[1]);
console.log("spot_market_vault      ", spotMarketVault[0].toBase58(), "bump", spotMarketVault[1]);
console.log("insurance_fund_vault   ", insuranceFundVault[0].toBase58(), "bump", insuranceFundVault[1]);
console.log("drift_signer           ", driftSigner[0].toBase58(), "bump", driftSigner[1]);

// example authority-derived PDAs (using the WSL wallet as a sample authority)
const sampleAuth = new PublicKey("FSUEW3LeovDwuKPJC8GatVhZjv18RhCbMKtoNgK3egZY");
const userStats = pda([enc("user_stats"), sampleAuth.toBuffer()]);
const ifStake = pda([enc("insurance_fund_stake"), sampleAuth.toBuffer(), u16le(market)]);
console.log("\n=== AUTHORITY-DERIVED (sample authority", sampleAuth.toBase58(), ") ===");
console.log("user_stats             ", userStats[0].toBase58(), "bump", userStats[1]);
console.log("insurance_fund_stake   ", ifStake[0].toBase58(), "bump", ifStake[1]);

// ── read SpotMarket on-chain ──
console.log("\n=== SpotMarket on-chain read ===");
const acct = await conn.getAccountInfo(spotMarket[0]);
if (!acct) { console.log("SpotMarket account NOT FOUND on chain"); }
else {
  const d = acct.data;
  console.log("owner:", acct.owner.toBase58(), "len:", d.length);
  console.log("disc:", JSON.stringify([...d.slice(0, 8)]));
  const pkAt = (o) => new PublicKey(d.slice(o, o + 32)).toBase58();
  const u128At = (o) => { let x = 0n; for (let i = 0; i < 16; i++) x |= BigInt(d[o + i]) << (8n * BigInt(i)); return x; };
  const i64At = (o) => d.readBigInt64LE(o);
  const u16At = (o) => d.readUInt16LE(o);
  console.log("market_index field (offset varies) — checking insurance_fund block:");
  console.log("  vault @304            ", pkAt(304));
  console.log("  total_shares @336     ", u128At(336).toString());
  console.log("  user_shares @352      ", u128At(352).toString());
  console.log("  shares_base @368      ", u128At(368).toString());
  console.log("  unstaking_period @384 ", i64At(384).toString(), "seconds =", (Number(i64At(384)) / 86400).toFixed(2), "days");
  console.log("  last_revenue_settle_ts @392", i64At(392).toString());
  console.log("  revenue_settle_period @400 ", i64At(400).toString());
  console.log("cross-check: spot_market.vault @104 =", pkAt(104), "(expect spot_market_vault PDA", spotMarketVault[0].toBase58(), ")");
  console.log("cross-check: insurance_fund.vault @304 =", pkAt(304), "(expect insurance_fund_vault PDA", insuranceFundVault[0].toBase58(), ")");
  // read insurance_fund_vault token balance
  const ifv = await conn.getAccountInfo(insuranceFundVault[0]);
  if (ifv) {
    const bal = ifv.data.readBigUInt64LE(64);
    console.log("insurance_fund_vault token balance (raw, 6dp):", bal.toString(), "=", (Number(bal) / 1e6).toFixed(2), "USDC");
  }
}

// ── PROBE the live program with each IF discriminator ──
console.log("\n=== LIVE PROBE: simulate each IF instruction discriminator ===");
const discs = {
  initialize_insurance_fund_stake: [187, 179, 243, 70, 248, 90, 92, 147],
  add_insurance_fund_stake: [251, 144, 115, 11, 222, 47, 62, 236],
  request_remove_insurance_fund_stake: [142, 70, 204, 92, 73, 106, 180, 52],
  cancel_request_remove_insurance_fund_stake: [97, 235, 78, 62, 212, 42, 241, 127],
  remove_insurance_fund_stake: [128, 166, 142, 9, 254, 187, 143, 174],
};
// Use a real, funded mainnet account as fee payer so the tx survives long enough to dispatch
// into Drift; disable sig verify + replace blockhash so we observe the program-level error, not
// a pre-flight account/sig error. The IF dispatch (fallback) check fires BEFORE account validation.
const feePayer = new PublicKey("FSUEW3LeovDwuKPJC8GatVhZjv18RhCbMKtoNgK3egZY"); // sample; replaced below if unfunded
// pick a known funded account: USDC mint owner authority is not signable; instead use the drift
// state account's update authority is unknown — simplest: use any system-owned funded account.
// We use Drift's own fee/insurance accounts are PDAs (can't sign). Use a large SOL holder.
const funded = new PublicKey("5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j"); // Raydium authority (funded, system-owned)
const blockhash = (await conn.getLatestBlockhash()).blockhash;
for (const [name, disc] of Object.entries(discs)) {
  const data = Buffer.concat([Buffer.from(disc), u16le(market), Buffer.alloc(8)]);
  const ix = new TransactionInstruction({
    programId: DRIFT,
    keys: [{ pubkey: funded, isSigner: true, isWritable: false }],
    data,
  });
  const msg = new TransactionMessage({
    payerKey: funded,
    recentBlockhash: blockhash,
    instructions: [ix],
  }).compileToV0Message();
  const tx = new VersionedTransaction(msg);
  try {
    const sim = await conn.simulateTransaction(tx, {
      sigVerify: false,
      replaceRecentBlockhash: true,
      commitment: "confirmed",
    });
    const err = sim.value.err;
    const logs = sim.value.logs || [];
    const relevant = logs.filter((l) => /Error|fallback|Fallback|0x|custom|invoke|consumed/i.test(l)).slice(0, 6);
    console.log(`\n- ${name}`);
    console.log("  err:", JSON.stringify(err));
    relevant.forEach((l) => console.log("  log:", l));
  } catch (e) {
    console.log(`\n- ${name}: simulate threw`, e.message);
  }
}
