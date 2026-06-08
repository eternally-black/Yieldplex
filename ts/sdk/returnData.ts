// current_value() returns its u64 via Solana program return-data. The dispatcher re-reports it, so
// reading a position's value is a `simulateTransaction` + a little-endian u64 decode — no account
// to deserialize, fully view-callable.
import { Connection, PublicKey, Transaction } from "@solana/web3.js";

/** Decode the base64 program return-data as a little-endian u64. */
export function decodeReturnedU64(base64?: string | null): bigint | null {
  if (!base64) return null;
  const buf = Buffer.from(base64, "base64");
  return buf.length >= 8 ? buf.readBigUInt64LE(0) : null;
}

/** Simulate a built (view) transaction and return the program returnData as a u64 (or null). */
export async function simulateReturnedU64(
  connection: Connection,
  tx: Transaction,
  feePayer: PublicKey,
): Promise<bigint | null> {
  tx.feePayer = feePayer;
  tx.recentBlockhash = (await connection.getLatestBlockhash()).blockhash;
  const sim = await connection.simulateTransaction(tx);
  const rd = sim.value.returnData;
  return rd ? decodeReturnedU64(rd.data[0]) : null;
}
