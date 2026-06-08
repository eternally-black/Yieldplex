// Verify Jupiter Perps facts for the JLP USDC liquidity adapter.
// - Reads all 5 custodies, decodes mint / token_account / doves_oracle / oracle.oracle_account at exact offsets.
// - Identifies the USDC custody (mint == USDC) and prints the 4 accounts add_liquidity2 needs.
// - Derives perpetuals / transfer_authority / event_authority PDAs and checks them on-chain.
// - Reads oracle account sizes + (best-effort) publish timestamps.
// Run: npx tsx scripts/verify-jup-perps.ts
import { readFileSync } from "node:fs";
import { PublicKey } from "@solana/web3.js";
import bs58 from "bs58";

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

const PROGRAM = new PublicKey("PERPHjGBqRHArX4DySjwM6UJHiR3sWAatqfdBS2qQJu");
const POOL = "5BUwFW4nRbftYTDMbgxykoFWqWHPzahFSNAaaaJtVKsq";
const JLP_MINT = "27G8MtK7VtTcCHkpASjSDdkWWYfoqT6ggEuKidVJidD4";
const USDC = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const CUSTODIES = [
  "7xS2gz2bTp3fwCC7knJvUWTEU9Tycczu6VhJYKgi1wdz",
  "AQCGyheWPLeo6Qp9WpYS9m3Qj479t7R636N9ey1rEjEn",
  "5Pv3gM9JrFFH883SWAhvJC9RPYmo8UNxuFtv5bMMALkm",
  "G18jKKXQwBbrHeiK3C9MRXhkHsLHf7XgCSisykV46EZa",
  "4vkNeXiYEUizLdrpdPS1eC2mccyM4NUPRtERrk6ZETkk",
];

// ---- Custody field offsets (Borsh, after 8-byte disc) ----
// pool:32 | mint:32 | token_account:32 | decimals:1 | is_stable:1 |
// oracle: OracleParams { oracle_account:32, oracle_type:enum(1), buffer:8, max_price_age_sec:4 } = 45
const OFF = {};
OFF.disc = 0;
OFF.pool = 8;
OFF.mint = OFF.pool + 32;            // 40
OFF.token_account = OFF.mint + 32;   // 72
OFF.decimals = OFF.token_account + 32; // 104
OFF.is_stable = OFF.decimals + 1;    // 105
OFF.oracle = OFF.is_stable + 1;      // 106  (OracleParams start)
OFF.oracle_account = OFF.oracle;     // 106  (first field of OracleParams)
OFF.oracle_type = OFF.oracle_account + 32; // 138
OFF.oracle_buffer = OFF.oracle_type + 1;   // 139
OFF.oracle_max_age = OFF.oracle_buffer + 8; // 147
OFF.oracle_end = OFF.oracle_max_age + 4;    // 151
// PricingParams: 6 * u64 = 48
OFF.pricing = OFF.oracle_end;        // 151
OFF.permissions = OFF.pricing + 48;  // 199 (7 bools)
OFF.target_ratio_bps = OFF.permissions + 7; // 206
OFF.assets = OFF.target_ratio_bps + 8;       // 214 (6 * u64 = 48)
OFF.funding_rate_state = OFF.assets + 48;    // 262 (u128+i64+u64 = 32)
OFF.bump = OFF.funding_rate_state + 32;      // 294
OFF.token_account_bump = OFF.bump + 1;       // 295
OFF.increase_position_bps = OFF.token_account_bump + 1; // 296
OFF.decrease_position_bps = OFF.increase_position_bps + 8; // 304
OFF.max_position_size_usd = OFF.decrease_position_bps + 8;  // 312
OFF.doves_oracle = OFF.max_position_size_usd + 8;           // 320

function pk(buf, off) { return bs58.encode(buf.subarray(off, off + 32)); }

async function rpc(method, params) {
  const r = await fetch(RPC, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }) });
  const j = await r.json();
  if (j.error) throw new Error(method + ": " + JSON.stringify(j.error));
  return j.result;
}

async function main() {
  console.log("RPC:", RPC.replace(/api-key=[^&]+/, "api-key=***"));
  console.log("Custody field offsets (after disc):", JSON.stringify({
    pool: OFF.pool, mint: OFF.mint, token_account: OFF.token_account,
    decimals: OFF.decimals, is_stable: OFF.is_stable,
    "oracle.oracle_account": OFF.oracle_account, "oracle.max_price_age_sec": OFF.oracle_max_age,
    doves_oracle: OFF.doves_oracle,
  }, null, 0));
  console.log();

  // --- derive PDAs ---
  const [perpetuals, perpBump] = PublicKey.findProgramAddressSync([Buffer.from("perpetuals")], PROGRAM);
  const [transferAuthority, taBump] = PublicKey.findProgramAddressSync([Buffer.from("transfer_authority")], PROGRAM);
  const [eventAuthority, eaBump] = PublicKey.findProgramAddressSync([Buffer.from("__event_authority")], PROGRAM);
  const [poolPda, poolBump] = PublicKey.findProgramAddressSync([Buffer.from("pool"), Buffer.from("Pool")], PROGRAM);
  console.log("PDAs:");
  console.log(`  perpetuals        seeds=[\"perpetuals\"]        bump=${perpBump}  -> ${perpetuals.toBase58()}`);
  console.log(`  transfer_authority seeds=[\"transfer_authority\"] bump=${taBump}  -> ${transferAuthority.toBase58()}`);
  console.log(`  event_authority   seeds=[\"__event_authority\"]   bump=${eaBump}  -> ${eventAuthority.toBase58()}`);
  console.log(`  (pool guess)      seeds=[\"pool\",\"Pool\"]         bump=${poolBump}  -> ${poolPda.toBase58()}  (target pool ${POOL})`);
  console.log();

  // --- check PDA accounts exist & owned by program ---
  const pdaCheck = await rpc("getMultipleAccounts", [[perpetuals.toBase58(), transferAuthority.toBase58(), eventAuthority.toBase58()], { encoding: "base64" }]);
  ["perpetuals", "transfer_authority", "event_authority"].forEach((n, i) => {
    const a = pdaCheck.value[i];
    console.log(`  on-chain ${n}: ${a ? "owner=" + a.owner + " len=" + Buffer.from(a.data[0], "base64").length : "MISSING"}`);
  });
  console.log();

  // --- read 5 custodies ---
  const res = await rpc("getMultipleAccounts", [CUSTODIES, { encoding: "base64" }]);
  let usdc = null;
  const oracleSet = new Set();
  for (let i = 0; i < CUSTODIES.length; i++) {
    const a = res.value[i];
    if (!a) { console.log(`[${i}] ${CUSTODIES[i]} MISSING`); continue; }
    const buf = Buffer.from(a.data[0], "base64");
    const mint = pk(buf, OFF.mint);
    const tokenAccount = pk(buf, OFF.token_account);
    const decimals = buf[OFF.decimals];
    const isStable = buf[OFF.is_stable];
    const oracleAccount = pk(buf, OFF.oracle_account);
    const maxAge = buf.readUInt32LE(OFF.oracle_max_age);
    const dovesOracle = pk(buf, OFF.doves_oracle);
    const sym = mint === USDC ? "  <== USDC" : "";
    console.log(`[${i}] custody ${CUSTODIES[i]} len=${buf.length}${sym}`);
    console.log(`     mint=${mint} decimals=${decimals} is_stable=${isStable}`);
    console.log(`     token_account(custody_token_account)=${tokenAccount}`);
    console.log(`     oracle.oracle_account(pythnet price)=${oracleAccount} max_price_age_sec=${maxAge}`);
    console.log(`     doves_oracle(custody_doves_price_account)=${dovesOracle}`);
    if (mint === USDC) usdc = { custody: CUSTODIES[i], mint, tokenAccount, oracleAccount, dovesOracle, decimals, isStable, maxAge };
    oracleSet.add(oracleAccount); oracleSet.add(dovesOracle);
  }
  console.log();
  if (usdc) {
    console.log("=== USDC CUSTODY SUMMARY ===");
    console.log(JSON.stringify(usdc, null, 2));
  } else {
    console.log("!! No USDC custody found (mint != USDC for all 5)");
  }
  console.log();

  // --- inspect oracle accounts: owner + length + best-effort publish time ---
  const oracleList = [...oracleSet];
  const oRes = await rpc("getMultipleAccounts", [oracleList, { encoding: "base64" }]);
  const clock = await rpc("getAccountInfo", ["SysvarC1ock11111111111111111111111111111111", { encoding: "base64" }]);
  let nowChain = null;
  if (clock && clock.value) {
    const cb = Buffer.from(clock.value.data[0], "base64");
    nowChain = cb.readBigInt64LE(32); // unix_timestamp at offset 32 in Clock sysvar
  }
  console.log("=== ORACLE ACCOUNTS (owner / len / candidate publish ts) ===");
  console.log("chain unix_timestamp (Clock sysvar):", nowChain ? nowChain.toString() : "n/a");
  for (let i = 0; i < oracleList.length; i++) {
    const a = oRes.value[i];
    if (!a) { console.log(`  ${oracleList[i]} MISSING`); continue; }
    const buf = Buffer.from(a.data[0], "base64");
    console.log(`  ${oracleList[i]} owner=${a.owner} len=${buf.length}`);
  }
}
main().catch((e) => { console.error("ERROR:", e.message); process.exit(2); });
