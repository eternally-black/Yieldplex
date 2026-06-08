// On-chain ground truth for the Jupiter Perps JLP pool. The Pool is a Borsh Anchor account with a
// leading String + Vec, so offsets are dynamic — parse sequentially: disc, name, custodies[], aum_usd.
// value(USDC) = jlp_balance * aum_usd / jlp_mint_supply (NAV). Confirms the aum_usd scale + custodies.
//
// Run: npx tsx scripts/inspect-jlp.ts   (MAINNET_RPC_URL from env or ./.env)
import { readFileSync } from "node:fs";

function loadRpc() {
  if (process.env.MAINNET_RPC_URL) return process.env.MAINNET_RPC_URL;
  for (const p of [".env", "../.env"]) {
    try {
      const m = readFileSync(p, "utf8").match(/^\s*MAINNET_RPC_URL\s*=\s*(.+)\s*$/m);
      if (m) return m[1].trim();
    } catch {}
  }
  throw new Error("MAINNET_RPC_URL not set");
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
  let z = 0;
  for (let i = 0; i < bytes.length && bytes[i] === 0; i++) z++;
  return "1".repeat(z) + digits.reverse().map((d) => B58[d]).join("");
}
const POOL = "5BUwFW4nRbftYTDMbgxykoFWqWHPzahFSNAaaaJtVKsq";
const JLP_MINT = "27G8MtK7VtTcCHkpASjSDdkWWYfoqT6ggEuKidVJidD4";

async function rpc(method, params) {
  const r = await fetch(RPC, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }) });
  const j = await r.json();
  if (j.error) throw new Error(method + ": " + JSON.stringify(j.error));
  return j.result;
}

async function main() {
  console.log("RPC:", RPC.replace(/api-key=[^&]+/, "api-key=***"), "\n");
  const res = await rpc("getMultipleAccounts", [[POOL, JLP_MINT], { encoding: "base64" }]);
  const pool = Buffer.from(res.value[0].data[0], "base64");
  const mint = Buffer.from(res.value[1].data[0], "base64");

  let o = 8; // skip discriminator
  const nameLen = pool.readUInt32LE(o); o += 4;
  const name = pool.subarray(o, o + nameLen).toString("utf8"); o += nameLen;
  const nCustodies = pool.readUInt32LE(o); o += 4;
  const custodies = [];
  for (let i = 0; i < nCustodies; i++) { custodies.push(b58encode(pool.subarray(o, o + 32))); o += 32; }
  const aumLo = pool.readBigUInt64LE(o); const aumHi = pool.readBigUInt64LE(o + 8);
  const aum_usd = aumLo + (aumHi << 64n); o += 16;

  const supply = mint.readBigUInt64LE(36);
  const decimals = mint[44];

  console.log(`Pool ${POOL}  name="${name}"  bytes=${pool.length}`);
  console.log(`aum_usd = ${aum_usd}   (offset ${8 + 4 + nameLen + 4 + nCustodies * 32})`);
  console.log(`JLP mint supply = ${supply}  decimals=${decimals}`);
  console.log(`custodies (${nCustodies}):`);
  custodies.forEach((c, i) => console.log(`  [${i}] ${c}`));
  // NAV per JLP and value of 25 JLP-worth.
  const price = Number(aum_usd) / Number(supply);
  console.log(`\nJLP price (aum_usd/supply) = ${price}  (expect ~4-5 if aum_usd is USD*1e6)`);
  const jlp = 5_000_000n; // 5 JLP (6dp)
  console.log(`value of ${jlp} JLP base units = ${(jlp * aum_usd) / supply}  (USDC 6dp)`);
}
main().catch((e) => { console.error("ERROR:", e.message); process.exit(2); });
