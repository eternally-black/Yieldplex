// On-chain ground truth for the Kamino USDC reserve (main market 7u3He…).
// Finds the USDC reserve via getProgramAccounts (disc + lending_market + liquidity.mint memcmp),
// decodes the value-math fields at the offsets derived from the vendored IDL, and computes the
// collateral exchange rate two ways so we can pick the one that matches the on-chain redemption.
//
// Run: node scripts/inspect-kamino.mjs   (MAINNET_RPC_URL from env or ./.env)
import { readFileSync } from "node:fs";

function loadRpc() {
  if (process.env.MAINNET_RPC_URL) return process.env.MAINNET_RPC_URL;
  for (const p of [".env", "../.env"]) {
    try {
      const m = readFileSync(p, "utf8").match(/^\s*MAINNET_RPC_URL\s*=\s*(.+)\s*$/m);
      if (m) return m[1].trim();
    } catch {}
  }
  throw new Error("MAINNET_RPC_URL not set (env or .env)");
}
const RPC = loadRpc();

const B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
function b58encode(bytes) {
  const digits = [0];
  for (const byte of bytes) {
    let carry = byte;
    for (let j = 0; j < digits.length; j++) { carry += digits[j] << 8; digits[j] = carry % 58; carry = (carry / 58) | 0; }
    while (carry > 0) { digits.push(carry % 58); carry = (carry / 58) | 0; }
  }
  let zeros = 0;
  for (let i = 0; i < bytes.length && bytes[i] === 0; i++) zeros++;
  return "1".repeat(zeros) + digits.reverse().map((d) => B58[d]).join("");
}

const KLEND = "KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD";
const MAIN_MARKET = "7u3HeHxYDLhnCoErrtycNokbQYbWGzLs6JSDqGAv5PfF";
const USDC = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const RESERVE_DISC = [43, 242, 204, 202, 26, 247, 59, 127];

async function rpc(method, params) {
  const r = await fetch(RPC, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }) });
  const j = await r.json();
  if (j.error) throw new Error(`${method}: ${JSON.stringify(j.error)}`);
  return j.result;
}

const pk = (buf, off) => b58encode(buf.subarray(off, off + 32));
const u64 = (buf, off) => buf.readBigUInt64LE(off);
const u128 = (buf, off) => buf.readBigUInt64LE(off) + (buf.readBigUInt64LE(off + 8) << 64n);

async function main() {
  console.log(`RPC: ${RPC.replace(/api-key=[^&]+/, "api-key=***")}\n`);
  const accts = await rpc("getProgramAccounts", [KLEND, {
    encoding: "base64",
    filters: [
      { memcmp: { offset: 0, bytes: b58encode(Buffer.from(RESERVE_DISC)) } },
      { memcmp: { offset: 32, bytes: MAIN_MARKET } },
      { memcmp: { offset: 128, bytes: USDC } },
    ],
  }]);
  if (!accts.length) throw new Error("no USDC reserve found under main market — offsets or market wrong");
  console.log(`found ${accts.length} matching reserve(s)\n`);

  for (const a of accts) {
    const buf = Buffer.from(a.account.data[0], "base64");
    const reserve = a.pubkey;
    console.log(`### Reserve ${reserve}   (bytes ${buf.length})`);
    console.log(`  lending_market            @32   = ${pk(buf, 32)}`);
    console.log(`  liquidity.mint            @128  = ${pk(buf, 128)}  (USDC expected)`);
    const supplyVault = pk(buf, 160);
    const feeVault = pk(buf, 192);
    console.log(`  liquidity.supply_vault    @160  = ${supplyVault}   (reserve_liquidity_supply)`);
    console.log(`  liquidity.fee_vault       @192  = ${feeVault}`);
    const avail = u64(buf, 224);
    const borrowedSf = u128(buf, 232);
    const mintDecimals = u64(buf, 272);
    const protocolFeesSf = u128(buf, 344);
    const referrerFeesSf = u128(buf, 360);
    const pendingReferrerFeesSf = u128(buf, 376);
    console.log(`  total_available_amount    @224  = ${avail}`);
    console.log(`  borrowed_amount_sf        @232  = ${borrowedSf}`);
    console.log(`  mint_decimals             @272  = ${mintDecimals}  (6 expected)`);
    console.log(`  accumulated_protocol_fees @344  = ${protocolFeesSf}`);
    console.log(`  accumulated_referrer_fees @360  = ${referrerFeesSf}`);
    console.log(`  pending_referrer_fees     @376  = ${pendingReferrerFeesSf}`);
    const ctokenMint = pk(buf, 2560);
    const ctokenSupply = u64(buf, 2592);
    const ctokenSupplyVault = pk(buf, 2600);
    console.log(`  collateral.mint           @2560 = ${ctokenMint}   (reserve_collateral_mint)`);
    console.log(`  collateral.mint_total_sup @2592 = ${ctokenSupply}`);
    console.log(`  collateral.supply_vault   @2600 = ${ctokenSupplyVault}`);

    // total_supply (Fraction, scale 2^60). Candidate fee sets to compare against on-chain redemption.
    const SCALE = 1n << 60n;
    const availSf = avail << 60n;
    const totalSf_full = availSf + borrowedSf - protocolFeesSf - referrerFeesSf - pendingReferrerFeesSf;
    const totalSf_protoOnly = availSf + borrowedSf - protocolFeesSf;
    const totalSf_noFees = availSf + borrowedSf;
    const valueOf = (sf, coll) => (ctokenSupply === 0n ? coll : (coll * sf) / (ctokenSupply * SCALE));
    const sample = 25_000_000n; // 25 USDC worth of cTokens, roughly
    console.log(`\n  exchange-rate sanity (liquidity per 1e6 cToken units):`);
    console.log(`    full-fees   : ${valueOf(totalSf_full, 1_000_000n)}  (avail+borrowed-proto-ref-pending)`);
    console.log(`    proto-only  : ${valueOf(totalSf_protoOnly, 1_000_000n)}`);
    console.log(`    no-fees     : ${valueOf(totalSf_noFees, 1_000_000n)}`);
    console.log(`    rate≈ totalLiquidity/ctokenSupply = ${Number((totalSf_full / ctokenSupply)) / Number(SCALE)}`);
    console.log(`  value of ${sample} cTokens (full-fees) = ${valueOf(totalSf_full, sample)}`);
    console.log("");
  }
}
main().catch((e) => { console.error("ERROR:", e.message); process.exit(2); });
