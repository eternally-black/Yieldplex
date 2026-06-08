// Nail the Chainlink Solana Transmissions layout for the syrupUSDC/USDC feed so the adapter can read
// the latest answer robustly (from live_cursor, not a frozen offset). Dumps header fields + locates
// the latest round's answer. Run: node scripts/inspect-chainlink.mjs
import { readFileSync } from "node:fs";
function loadRpc() {
  if (process.env.MAINNET_RPC_URL) return process.env.MAINNET_RPC_URL;
  for (const p of [".env", "../.env"]) { try { const m = readFileSync(p, "utf8").match(/^\s*MAINNET_RPC_URL\s*=\s*(.+)\s*$/m); if (m) return m[1].trim(); } catch {} }
  throw new Error("no rpc");
}
const RPC = loadRpc();
const FEED = "CpNyiFt84q66665Kx64bobxZuMgZ2EecrhAJs1HikS2T";
async function rpc(m, p) { const r = await fetch(RPC, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: m, params: p }) }); const j = await r.json(); if (j.error) throw new Error(JSON.stringify(j.error)); return j.result; }
function i128le(buf, off) { let n = 0n; for (let i = 15; i >= 0; i--) n = (n << 8n) | BigInt(buf[off + i]); if (n >= (1n << 127n)) n -= (1n << 128n); return n; }

const r = await rpc("getAccountInfo", [FEED, { encoding: "base64" }]);
const b = Buffer.from(r.value.data[0], "base64");
console.log(`owner=${r.value.owner}  bytes=${b.length}`);
// Chainlink store Transmissions header (after 8-byte disc): version u8, state u8, owner[32],
// proposed_owner[32], writer[32], description[32], decimals u8, flagging_threshold u32,
// latest_round_id u32, granularity u8, live_length u32, live_cursor u32, historical_cursor u32.
// repr(C) aligned offsets (account-level, struct @8): decimals@138, pad, flagging@140,
// latest_round_id@144, granularity@148, pad, live_length@152, live_cursor@156, historical@160.
const decimals = b[138];
const flagging = b.readUInt32LE(140);
const latest_round_id = b.readUInt32LE(144);
const granularity = b[148];
const live_length = b.readUInt32LE(152);
const live_cursor = b.readUInt32LE(156);
const historical_cursor = b.readUInt32LE(160);
const desc = b.subarray(8 + 2 + 96, 8 + 2 + 96 + 32).toString("utf8").replace(/\0+$/, "");
console.log(`description="${desc}"  decimals=${decimals}  latest_round_id=${latest_round_id}`);
console.log(`granularity=${granularity}  live_length=${live_length}  live_cursor=${live_cursor}  historical_cursor=${historical_cursor}`);
const RING = 200; // empirically the first transmission (answer of entry 0 lands at 216)
console.log(`ring start (empirical) = ${RING}`);
// Transmission { slot u64, timestamp u32, _pad u32, answer i128, _pad u64, _pad u64 } = 48 bytes.
const ENTRY = 48;
const idx = (live_cursor + live_length - 1) % live_length; // latest written slot
const off = RING + idx * ENTRY;
console.log(`latest entry idx=${idx}  offset=${off}`);
console.log(`  slot=${b.readBigUInt64LE(off)}  timestamp=${b.readUInt32LE(off + 8)}  answer=${i128le(b, off + 16)}`);
console.log(`  => price = ${Number(i128le(b, off + 16)) / 10 ** decimals} USDC/syrup`);
// sanity: scan for 1167864-ish answers to confirm ENTRY/RING
const target = 1167864n;
for (let o = RING; o + 16 <= b.length; o += 8) { if (i128le(b, o) === target) console.log(`  found answer ${target} at offset ${o} (entry+16 => entry@${o - 16}, rel ${o - 16 - RING}, /48=${(o - 16 - RING) / 48})`); }
