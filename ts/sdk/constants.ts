// Yield Adapter Standard — shared constants (mirror of crates/ya-interface/src/constants.rs).
import { PublicKey, SystemProgram } from "@solana/web3.js";
import * as anchor from "@anchor-lang/core";

/** Base asset of the reference build: USDC (6 decimals). */
export const USDC_MINT = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
export const USDC_DECIMALS = 6;

export const TOKEN_PROGRAM_ID = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
export const ASSOCIATED_TOKEN_PROGRAM_ID = new PublicKey("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
export const SYSTEM_PROGRAM_ID = SystemProgram.programId;

/** Canonical program ids (Anchor.toml / declare_id!). */
export const REGISTRY_PROGRAM_ID = new PublicKey("3ehQoDePP3eULnSKxgHc6DvLAwEQNeVHvJYzWPXoQyUD");
export const DISPATCHER_PROGRAM_ID = new PublicKey("2aY1hBVBJJmX8uSgB4aqhuS2xeDaGCc3d55KE2Mbvvgs");

/** Standard PDA seed prefixes — identical across every adapter. */
export const SEED_POSITION = Buffer.from("position");
export const SEED_VAULT_AUTHORITY = Buffer.from("vault_authority");
export const SEED_TICKET = Buffer.from("ticket");
export const SEED_REGISTRY = Buffer.from("registry");
export const SEED_ADAPTER = Buffer.from("adapter");

/** Standard instruction names (sha256("global:<name>")[..8] derives the discriminator). */
export const IX = {
  INITIALIZE_POSITION: "initialize_position",
  DEPOSIT: "deposit",
  WITHDRAW: "withdraw",
  SETTLE_WITHDRAWAL: "settle_withdrawal",
  CURRENT_VALUE: "current_value",
  CANCEL_WITHDRAWAL: "cancel_withdrawal",
} as const;

/** USDC base units from a human amount: usdc(25) -> BN(25_000_000). */
export function usdc(amount: number): anchor.BN {
  return new anchor.BN(Math.round(amount * 10 ** USDC_DECIMALS));
}
