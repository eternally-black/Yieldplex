// probe:drift-if — evidence that a live Drift Insurance-Fund-staking CPI is impossible.
// Simulates each IF-staking discriminator against the LIVE deployed Drift program and shows it does
// not dispatch them (InstructionFallbackNotFound / error 101).
//
// HONEST FRAMING (read docs/adapters/drift-if.md): the AUTHORITATIVE proof is the source — these
// instructions are commented out of Drift's #[program] in drift-labs/protocol-v2 programs/drift/
// src/lib.rs (~lines 796-880). A blind simulation returns 101 for many instructions when accounts
// are incomplete, so this probe is corroborating, not sole, evidence. It confirms our adapter's CPI
// would fail on the deployed program today — hence the spec-correct adapter + the two-phase lifecycle
// proven on the cooldown stand-in (tests/adapters/drift-if.spec.ts).
import { readFileSync } from "node:fs";
import { Connection, Keypair, PublicKey, Transaction, TransactionInstruction } from "@solana/web3.js";
import * as os from "node:os";
import * as path from "node:path";

function loadRpc() {
  if (process.env.MAINNET_RPC_URL) return process.env.MAINNET_RPC_URL;
  const m = readFileSync(".env", "utf8").match(/^\s*MAINNET_RPC_URL\s*=\s*(.+)\s*$/m);
  if (m) return m[1].trim();
  throw new Error("MAINNET_RPC_URL not set");
}
const conn = new Connection(loadRpc(), "confirmed");
const wallet = Keypair.fromSecretKey(
  Uint8Array.from(JSON.parse(readFileSync(path.join(os.homedir(), ".config/solana/id.json"), "utf8"))),
);
const DRIFT = new PublicKey("dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH");
// IF-staking discriminators (cross-checked against the IDL in crates/ya-interface/src/cpi.rs).
const IXS = {
  initialize_insurance_fund_stake: [187, 179, 243, 70, 248, 90, 92, 147],
  add_insurance_fund_stake: [251, 144, 115, 11, 222, 47, 62, 236],
  request_remove_insurance_fund_stake: [142, 70, 204, 92, 73, 106, 180, 52],
  remove_insurance_fund_stake: [128, 166, 142, 9, 254, 187, 143, 174],
  cancel_request_remove_insurance_fund_stake: [97, 235, 78, 62, 212, 42, 241, 127],
};

console.log("Probing the LIVE Drift program", DRIFT.toBase58(), "for IF-staking dispatch:\n");
let allRejected = true;
for (const [name, disc] of Object.entries(IXS)) {
  const ix = new TransactionInstruction({
    programId: DRIFT,
    keys: [{ pubkey: wallet.publicKey, isSigner: true, isWritable: false }],
    data: Buffer.from([...disc, 0, 0]), // disc + market_index u16
  });
  const tx = new Transaction().add(ix);
  tx.feePayer = wallet.publicKey;
  tx.recentBlockhash = (await conn.getLatestBlockhash()).blockhash;
  const sim = await conn.simulateTransaction(tx).catch((e) => ({ value: { err: String(e), logs: [] } }));
  const err = JSON.stringify(sim.value.err);
  const log = (sim.value.logs || []).find((l) => /Fallback|Error|failed|invalid/i.test(l)) || "";
  // Any error means the live program did NOT let us IF-stake. A clean InstructionFallbackNotFound
  // (101) is the textbook signal; other errors (e.g. AccountNotFound from a sim fee-payer) still
  // mean "not executable". Either way no IF-staking instruction succeeds.
  const notExecuted = sim.value.err !== null;
  const isFallback = err.includes("101") || /FallbackNotFound/i.test(log);
  if (!notExecuted) allRejected = false;
  console.log(`  ${name}: ${notExecuted ? "REJECTED" : "EXECUTED?!"}  err=${err}${isFallback ? "  [InstructionFallbackNotFound]" : ""}${log ? "  log=" + log : ""}`);
}
console.log(`\n${allRejected ? "No IF-staking instruction executes on the live deployed Drift program (all rejected)." : "Unexpected: an IF-staking instruction executed."}`);
console.log("AUTHORITATIVE proof of removal: drift-labs/protocol-v2 programs/drift/src/lib.rs ~796-880");
console.log("(the *_insurance_fund_stake handlers are commented out of #[program]). The spec-correct");
console.log("adapter (ya-adapter-drift-if) will execute the instant Drift re-enables those exports;");
console.log("its two-phase lifecycle is proven on ya-cooldown-standin (tests/adapters/drift-if.spec.ts).");
process.exit(allRejected ? 0 : 1);
