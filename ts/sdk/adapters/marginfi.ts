// MarginFi v2 USDC adapter — ported from tests/adapters/marginfi.spec.ts (verified working on fork).
// Position lives as asset_shares inside a program-owned marginfi_account PDA. value = shares ×
// asset_share_value (I80F48). See docs/adapters/marginfi.md.
import * as anchor from "@anchor-lang/core";
import { PublicKey } from "@solana/web3.js";
import { AdapterDef, PositionContext, meta } from "./types";
import { subVaultPda } from "../pdas";
import { USDC_MINT, TOKEN_PROGRAM_ID, SYSTEM_PROGRAM_ID } from "../constants";

export const MARGINFI_ADAPTER_PROGRAM_ID = new PublicKey("36CgQYZFxZQHzyMrn3NJRXR9jsVoYH44WitqGohoBGoi");

// Verified mainnet accounts (main group USDC bank).
const MARGINFI = new PublicKey("MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA");
const GROUP = new PublicKey("4qp6Fx6tnZkY5Wropq9wUYgtFxXKwE6viZxFHg3rdAG8");
const BANK = new PublicKey("2s37akK2eyBbp8DZgCm7RtsaEz8eJP3Nxd4urLHQv7yB");
const LIQ_VAULT = new PublicKey("7jaiZR5Sk8hdYN9MxTpczTcwbWpb5WEoxSANuUwveuat");
const ORACLE = new PublicKey("Dpw1EAVrSB1ibxiDQyTAW6Zip3J4Btk2x4SgApQCeFbX"); // Pyth push, oracle_keys[0]
const LIQ_VAULT_AUTH = PublicKey.findProgramAddressSync(
  [Buffer.from("liquidity_vault_auth"), BANK.toBuffer()], MARGINFI)[0];

const vaultUsdc = (ctx: PositionContext) => subVaultPda(ctx.programId, "vault_usdc", ctx.position);
const marginfiAccount = (ctx: PositionContext) => subVaultPda(ctx.programId, "marginfi_account", ctx.position);

export const marginfiAdapter: AdapterDef = {
  label: "marginfi",
  programId: MARGINFI_ADAPTER_PROGRAM_ID,
  baseMint: USDC_MINT,
  isInstant: true,
  computeUnits: 800_000,
  vaultTokenAccount: vaultUsdc,
  depositRemaining: (ctx) => [
    meta(marginfiAccount(ctx), true),
    meta(GROUP, false),
    meta(BANK, true),
    meta(LIQ_VAULT, true),
    meta(MARGINFI, false),
  ],
  valueRemaining: (ctx) => [meta(marginfiAccount(ctx), false), meta(BANK, false)],
  withdrawRemaining: (ctx) => [
    meta(ctx.ticket, true),
    meta(marginfiAccount(ctx), true),
    meta(GROUP, false),
    meta(BANK, true),
    meta(LIQ_VAULT_AUTH, true),
    meta(LIQ_VAULT, true),
    meta(ORACLE, false),
    meta(MARGINFI, false),
  ],
  async buildInitPosition(program, ctx) {
    return program.methods
      .initializePosition()
      .accountsPartial({
        position: ctx.position,
        vaultAuthority: ctx.vaultAuthority,
        baseMint: ctx.baseMint,
        vaultUsdc: vaultUsdc(ctx),
        marginfiAccount: marginfiAccount(ctx),
        marginfiGroup: GROUP,
        marginfiProgram: MARGINFI,
        owner: ctx.owner,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SYSTEM_PROGRAM_ID,
      })
      .instruction();
  },
};
