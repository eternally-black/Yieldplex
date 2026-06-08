// An AdapterDef is the ONLY adapter-specific surface in the SDK: the protocol account order for
// deposit/withdraw/current_value (after the standard 9-account prefix) plus the adapter's own
// initialize_position. Everything else (routing, decoding, value-reading, tickets) is uniform.
import * as anchor from "@anchor-lang/core";
import { AccountMeta, PublicKey, TransactionInstruction } from "@solana/web3.js";

/** Derived PDAs for one (adapter, owner, base_mint) position — passed to every builder. */
export interface PositionContext {
  /** Adapter program id. */
  programId: PublicKey;
  owner: PublicKey;
  baseMint: PublicKey;
  position: PublicKey;
  vaultAuthority: PublicKey;
  ticket: PublicKey;
}

export interface AdapterDef {
  label: string;
  /** Adapter program id (Anchor.toml / declare_id!). */
  programId: PublicKey;
  baseMint: PublicKey;
  /** Instant adapters settle in `withdraw`; two-phase open a Pending ticket then `settle`. */
  isInstant: boolean;
  /** Nominal cooldown for two-phase adapters (seconds); read from chain in practice. */
  cooldownSeconds?: number;
  /** Compute-unit limit prepended to each route (protocol CPIs that revalue pools need headroom). */
  computeUnits?: number;
  /** Vault USDC token account (prefix #3). Omitted for non-token adapters (e.g. the cooldown stand-in). */
  vaultTokenAccount?(ctx: PositionContext): PublicKey;
  /** Owner token account (prefix #5). Defaults to ATA(base_mint, owner) in the client. */
  ownerTokenAccount?(ctx: PositionContext): PublicKey;
  /** Protocol accounts appended after the prefix for deposit. */
  depositRemaining(ctx: PositionContext): AccountMeta[];
  /** Protocol accounts for withdraw — MUST start with the withdrawal ticket. */
  withdrawRemaining(ctx: PositionContext): AccountMeta[];
  /** Accounts for current_value (usually just the protocol pool/reserve/oracle). */
  valueRemaining(ctx: PositionContext): AccountMeta[];
  /** Build the adapter-specific initialize_position instruction. */
  buildInitPosition(program: anchor.Program, ctx: PositionContext): Promise<TransactionInstruction>;
}

/** AccountMeta helper: meta(key, writable, signer?). */
export const meta = (pubkey: PublicKey, isWritable: boolean, isSigner = false): AccountMeta =>
  ({ pubkey, isSigner, isWritable });
