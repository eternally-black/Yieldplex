// Surfnet cheatcodes — thin wrappers over the surfnet JSON-RPC extensions.
// Param shapes follow the surfpool docs; the ones the adapters actually use (setTokenAccount,
// timeTravel, setAccount) are exercised against live surfnet in the M5 adapter fork-tests.
import { Connection, PublicKey } from "@solana/web3.js";

const RPC = process.env.ANCHOR_PROVIDER_URL || "http://127.0.0.1:8899";
const USDC = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

export async function cheat(method: string, params: any[]): Promise<any> {
  const r = await fetch(RPC, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
  });
  const j: any = await r.json();
  if (j.error) throw new Error(`${method}: ${JSON.stringify(j.error)}`);
  return j.result;
}

/** Give `owner` a USDC SPL token account with `amount` base units (surfnet creates the ATA). */
export async function fundUsdc(owner: PublicKey, amount: bigint, mint = USDC): Promise<void> {
  // surfnet_setTokenAccount expects `amount` as a JSON u64 (number), not a string.
  await cheat("surfnet_setTokenAccount", [
    owner.toBase58(),
    mint,
    { amount: Number(amount), delegate: null, state: "initialized" },
    TOKEN_PROGRAM,
  ]);
}

/** Overwrite an arbitrary account's lamports/data/owner (e.g. patch a stale oracle/utilization). */
export async function setAccount(pubkey: PublicKey, fields: Record<string, unknown>): Promise<void> {
  await cheat("surfnet_setAccount", [pubkey.toBase58(), fields]);
}

/** Read the on-chain Clock unix timestamp. */
export async function nowTs(connection: Connection): Promise<number> {
  const slot = await connection.getSlot();
  const t = await connection.getBlockTime(slot);
  return t ?? Math.floor(Date.now() / 1000);
}

/** Advance surfnet time by `seconds` (for cooldown/lifecycle tests). absoluteTimestamp is in ms. */
export async function warpForwardSeconds(connection: Connection, seconds: number): Promise<void> {
  const target = (await nowTs(connection)) + seconds;
  await cheat("surfnet_timeTravel", [{ absoluteTimestamp: target * 1000 }]);
}
