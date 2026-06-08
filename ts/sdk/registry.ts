// Registry reads — enumerate the on-chain AdapterEntry PDAs (what an integrator queries to discover
// which adapters are live and Active). Uses the SDK's own AdapterEntry decoder (no IDL needed).
import { Connection, PublicKey } from "@solana/web3.js";
import { REGISTRY_PROGRAM_ID } from "./constants";
import { decodeAdapterEntry, AdapterEntry, ADAPTER_ENTRY_SIZE } from "./decode";

export interface AdapterEntryRecord extends AdapterEntry {
  /** The entry PDA address `[b"adapter", program_id]`. */
  address: PublicKey;
}

/** List every registered adapter (any status). Filter `.status === "active"` for routable ones. */
export async function listAdapters(
  connection: Connection,
  registryProgramId: PublicKey = REGISTRY_PROGRAM_ID,
): Promise<AdapterEntryRecord[]> {
  const accounts = await connection.getProgramAccounts(registryProgramId, {
    filters: [{ dataSize: ADAPTER_ENTRY_SIZE }],
  });
  const out: AdapterEntryRecord[] = [];
  for (const { pubkey, account } of accounts) {
    try {
      out.push({ address: pubkey, ...decodeAdapterEntry(account.data) });
    } catch {
      // not an AdapterEntry (discriminator mismatch) — skip.
    }
  }
  return out;
}

/** Only the Active (routable) adapters. */
export async function listActiveAdapters(
  connection: Connection,
  registryProgramId: PublicKey = REGISTRY_PROGRAM_ID,
): Promise<AdapterEntryRecord[]> {
  return (await listAdapters(connection, registryProgramId)).filter((e) => e.status === "active");
}
