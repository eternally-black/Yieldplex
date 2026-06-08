// On-chain ground truth for the MarginFi main-group USDC bank.
// Finds the Bank (disc + mint==USDC + group==main), decodes asset_share_value (I80F48) + the
// liquidity vault + bumps, and verifies the field offsets used by the adapter.
//
// Run: node scripts/inspect-marginfi.mjs   (MAINNET_RPC_URL from env or ./.env)
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

const MFI = "MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA";
const USDC = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const MAIN_GROUP = "4qp6Fx6tnZkY5Wropq9wUYgtFxXKwE6viZxFHg3rdAG8";
const BANK_DISC = [142, 49, 166, 242, 50, 66, 97, 188];

async function rpc(method, params) {
  const r = await fetch(RPC, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }) });
  const j = await r.json();
  if (j.error) throw new Error(method + ": " + JSON.stringify(j.error));
  return j.result;
}
const pk = (buf, off) => b58encode(buf.subarray(off, off + 32));
const i128 = (buf, off) => {
  let n = 0n;
  for (let i = 15; i >= 0; i--) n = (n << 8n) | BigInt(buf[off + i]);
  if (n >= (1n << 127n)) n -= (1n << 128n);
  return n;
};

async function main() {
  console.log("RPC:", RPC.replace(/api-key=[^&]+/, "api-key=***"), "\n");
  const accts = await rpc("getProgramAccounts", [MFI, {
    encoding: "base64",
    filters: [
      { memcmp: { offset: 0, bytes: b58encode(Buffer.from(BANK_DISC)) } },
      { memcmp: { offset: 8, bytes: USDC } },
      { memcmp: { offset: 41, bytes: MAIN_GROUP } },
    ],
  }]);
  console.log("main-group USDC banks:", accts.length);
  for (const a of accts) {
    const buf = Buffer.from(a.account.data[0], "base64");
    const asv = i128(buf, 80);
    console.log("### Bank " + a.pubkey + "  bytes=" + buf.length);
    console.log("  mint            @8   = " + pk(buf, 8) + " (USDC expected)");
    console.log("  mint_decimals   @40  = " + buf[40]);
    console.log("  group           @41  = " + pk(buf, 41) + " (main group)");
    console.log("  asset_share_value@80 = " + asv.toString() + "  (I80F48; ~= " + (Number(asv) / 2 ** 48).toFixed(8) + " token/share)");
    console.log("  liquidity_vault @112 = " + pk(buf, 112));
    console.log("  liq_vault_bump  @144 = " + buf[144] + "   liq_vault_authority_bump @145 = " + buf[145]);
    // value of 25 USDC worth of shares (shares ~= 25e6/asv*2^48): sanity that asv ~ 1.0x
    const shares = (25_000_000n << 48n) * (1n << 48n) / asv; // inverse: deposit_tokens -> shares
    const value = (shares * asv) >> 96n;
    console.log("  round-trip check: 25e6 USDC -> shares " + shares + " -> value " + value + " (expect ~25000000)");
  }
}
main().catch((e) => { console.error("ERROR:", e.message); process.exit(2); });
