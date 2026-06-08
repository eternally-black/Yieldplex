// M0 address verification — reads every §8 address from chain (read-only) and asserts
// owner / executable / mint decimals. Resolves the Maple entry path by reading the
// syrupUSDC mint_authority and the program that owns it. No deps (Node 20 global fetch).
//
// Run: npx tsx scripts/verify-addresses.ts   (MAINNET_RPC_URL from env or ./.env)

import { readFileSync } from "node:fs";

// ---- load RPC url ----
function loadRpc() {
  if (process.env.MAINNET_RPC_URL) return process.env.MAINNET_RPC_URL;
  for (const p of [".env", "../.env"]) {
    try {
      const txt = readFileSync(p, "utf8");
      const m = txt.match(/^\s*MAINNET_RPC_URL\s*=\s*(.+)\s*$/m);
      if (m) return m[1].trim();
    } catch {}
  }
  throw new Error("MAINNET_RPC_URL not set (env or .env)");
}
const RPC = loadRpc();

// ---- base58 ----
const B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
function b58encode(bytes) {
  const digits = [0];
  for (const byte of bytes) {
    let carry = byte;
    for (let j = 0; j < digits.length; j++) {
      carry += digits[j] << 8;
      digits[j] = carry % 58;
      carry = (carry / 58) | 0;
    }
    while (carry > 0) { digits.push(carry % 58); carry = (carry / 58) | 0; }
  }
  let zeros = 0;
  for (let i = 0; i < bytes.length && bytes[i] === 0; i++) zeros++;
  return "1".repeat(zeros) + digits.reverse().map((d) => B58[d]).join("");
}

// ---- known programs ----
const TOKEN = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const TOKEN22 = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
const BPF_UP = "BPFLoaderUpgradeab1e11111111111111111111111";
const SYSTEM = "11111111111111111111111111111111";
const known = { [TOKEN]: "SPL-Token", [TOKEN22]: "Token-2022", [BPF_UP]: "BPFLoaderUpgradeable", [SYSTEM]: "System" };

// ---- targets (§8) ----
const KLEND = "KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD";
const PERPS = "PERPHjGBqRHArX4DySjwM6UJHiR3sWAatqfdBS2qQJu";
const SYRUP_MINT = "AvZZF1YaZDziPY2RCK4oJrRVrbN3mTD9NL24hPeaZeUj";

const targets = [
  { label: "USDC mint", addr: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", kind: "mint", expectOwner: [TOKEN], expectDecimals: 6 },
  { label: "Kamino klend program", addr: KLEND, kind: "program", expectOwner: [BPF_UP] },
  { label: "Kamino main market", addr: "7u3HeHxYDLhnCoErrtycNokbQYbWGzLs6JSDqGAv5PfF", kind: "account", expectOwner: [KLEND] },
  { label: "Jupiter Perps program", addr: PERPS, kind: "program", expectOwner: [BPF_UP] },
  { label: "JLP pool account", addr: "5BUwFW4nRbftYTDMbgxykoFWqWHPzahFSNAaaaJtVKsq", kind: "account", expectOwner: [PERPS] },
  { label: "JLP mint", addr: "27G8MtK7VtTcCHkpASjSDdkWWYfoqT6ggEuKidVJidD4", kind: "mint", expectOwner: [TOKEN, TOKEN22] },
  { label: "MarginFi v2 program", addr: "MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA", kind: "program", expectOwner: [BPF_UP] },
  // NOTE: brief §8 listed dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcozatH which does NOT exist on chain.
  // Canonical Drift v2 program (official docs + solana verified build) ends VPcn33UH:
  { label: "Drift program", addr: "dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH", kind: "program", expectOwner: [BPF_UP] },
  { label: "syrupUSDC mint", addr: SYRUP_MINT, kind: "mint", expectOwner: [TOKEN, TOKEN22], expectDecimals: 6 },
  { label: "syrupUSDC CCIP token pool", addr: "HrTBpF3LqSxXnjnYdR4htnBLyMHNZ6eNaDZGPundvHbm", kind: "account", expectOwner: null },
];

async function rpc(method, params) {
  const r = await fetch(RPC, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
  });
  const j = await r.json();
  if (j.error) throw new Error(`${method}: ${JSON.stringify(j.error)}`);
  return j.result;
}

function decodeMint(buf) {
  const out = {};
  const maOpt = buf.readUInt32LE(0);
  out.mintAuthority = maOpt === 1 ? b58encode(buf.subarray(4, 36)) : null;
  out.supply = buf.readBigUInt64LE(36).toString();
  out.decimals = buf[44];
  out.isInitialized = buf[45] === 1;
  const faOpt = buf.readUInt32LE(46);
  out.freezeAuthority = faOpt === 1 ? b58encode(buf.subarray(50, 82)) : null;
  return out;
}

let fails = 0;
const pass = (c) => (c ? "PASS" : ((fails++), "FAIL"));

async function main() {
  console.log(`RPC: ${RPC.replace(/api-key=[^&]+/, "api-key=***")}\n`);
  const res = await rpc("getMultipleAccounts", [targets.map((t) => t.addr), { encoding: "base64" }]);
  const vals = res.value;

  let syrupMintAuthority = null;
  for (let i = 0; i < targets.length; i++) {
    const t = targets[i];
    const v = vals[i];
    console.log(`### ${t.label}  (${t.addr})`);
    if (!v) { console.log(`  ${pass(false)} account not found on chain\n`); continue; }
    const ownerName = known[v.owner] || v.owner;
    console.log(`  owner: ${ownerName}   executable: ${v.executable}   lamports: ${v.lamports}   bytes: ${v.space}`);
    if (t.expectOwner) console.log(`  owner check: ${pass(t.expectOwner.includes(v.owner))} (expected ${t.expectOwner.map((o) => known[o] || o).join(" | ")})`);
    if (t.kind === "program") console.log(`  executable check: ${pass(v.executable === true)}`);
    if (t.kind === "mint") {
      const buf = Buffer.from(v.data[0], "base64");
      const m = decodeMint(buf);
      console.log(`  decimals: ${m.decimals}   mintAuthority: ${m.mintAuthority}   freezeAuthority: ${m.freezeAuthority}   initialized: ${m.isInitialized}`);
      if (t.expectDecimals !== undefined) console.log(`  decimals check: ${pass(m.decimals === t.expectDecimals)} (expected ${t.expectDecimals})`);
      if (v.owner === TOKEN22) console.log(`  NOTE: Token-2022 mint — extension validation required (§23): screen PermanentDelegate / FreezeAuthority`);
      if (t.addr === SYRUP_MINT) syrupMintAuthority = m.mintAuthority;
    }
    console.log("");
  }

  // ---- Maple entry-path resolution: who owns the syrupUSDC mint_authority? ----
  console.log("=== Maple entry-path resolution (§7.3) ===");
  if (syrupMintAuthority) {
    console.log(`syrupUSDC mint_authority = ${syrupMintAuthority}`);
    const ma = await rpc("getAccountInfo", [syrupMintAuthority, { encoding: "base64" }]);
    if (ma.value) {
      const owner = ma.value.owner;
      console.log(`mint_authority account owner (controlling program) = ${known[owner] || owner}   executable: ${ma.value.executable}   bytes: ${ma.value.space}`);
      console.log(`Interpretation: if owned by a CCIP BurnMint token-pool program -> no native Solana mint -> SWAP path.`);
      console.log(`                if owned by a Maple program -> investigate a native deposit/mint instruction (preferred).`);
    } else {
      console.log(`mint_authority account not found (may be a PDA off-curve with no lamports of its own).`);
    }
  } else {
    console.log("syrupUSDC mint_authority is None (fixed-supply) — investigate.");
  }

  console.log(`\n==== ${fails === 0 ? "ALL CHECKS PASSED" : fails + " CHECK(S) FAILED"} ====`);
  process.exit(fails === 0 ? 0 : 1);
}
main().catch((e) => { console.error("ERROR:", e.message); process.exit(2); });
