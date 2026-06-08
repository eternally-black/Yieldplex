// Maple syrupUSDC adapter — ported from tests/adapters/maple.spec.ts (verified working on fork).
// Swap-and-hold via one Orca Whirlpool swap (A=syrupUSDC / B=USDC): deposit buys syrup (a_to_b=false,
// ascending ticks), withdraw sells it (a_to_b=true, descending ticks). value = syrup × Chainlink rate
// (never the pool quote; fail-closed on a stale feed). See docs/adapters/maple.md.
import * as anchor from "@anchor-lang/core";
import { PublicKey } from "@solana/web3.js";
import { AdapterDef, PositionContext, meta } from "./types";
import { subVaultPda } from "../pdas";
import { USDC_MINT, TOKEN_PROGRAM_ID, SYSTEM_PROGRAM_ID } from "../constants";

export const MAPLE_ADAPTER_PROGRAM_ID = new PublicKey("Ck9mwpX9kAjycbtN7jhD3s9xdHzUS2dwuV43g3BuBnD");

// Verified mainnet accounts.
const ORCA = new PublicKey("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc");
const POOL = new PublicKey("6fteKNvMdv7tYmBoJHhj1jx6rHcEwC6RdSEmVpyS613J");
const SYRUP = new PublicKey("AvZZF1YaZDziPY2RCK4oJrRVrbN3mTD9NL24hPeaZeUj");
const VAULT_A = new PublicKey("FM2RuqFYo9umA1yc5FyQn6pSDZJZ1MXAdaekJZ4dQCvi"); // syrup vault (A)
const VAULT_B = new PublicKey("Fw6Xr45rBBrXbWJd5ZbSg44kacrKRLef4rHkZ8gWC5Ab"); // USDC vault (B)
const ORACLE = new PublicKey("H7j5FQpwTUMwxrWeuyrLr5Z9oHsPFiaRqNaERVsuE1c8"); // [b"oracle", whirlpool]
const CHAINLINK = new PublicKey("CpNyiFt84q66665Kx64bobxZuMgZ2EecrhAJs1HikS2T");
const BUY_TICKS = [
  "4yRC9NUHB2dwxfZyrqA8dDqH8GkcUVKU5F7W3ZPnbQtd",
  "AdLyWhs7xrwkBFCYEo3n9BiwgXMZzXMefh8K9wMWoy1j",
  "AofDEAkfQxcyeochNwxyQehYm6SpL3qrtxm7ZEZtPptp",
].map((s) => new PublicKey(s));
const SELL_TICKS = [
  "4yRC9NUHB2dwxfZyrqA8dDqH8GkcUVKU5F7W3ZPnbQtd",
  "9qUH5rp6Xw7NqghvbR9eQu6xTjEu5QTCHMbjdiiDVd5S",
  "BQ95wDV5A7z4c9cExYMWE2KvcqhbdjoxXcoQ88erFtyH",
].map((s) => new PublicKey(s));

const vaultUsdc = (ctx: PositionContext) => subVaultPda(ctx.programId, "vault_usdc", ctx.position);
const vaultSyrup = (ctx: PositionContext) => subVaultPda(ctx.programId, "vault_syrup", ctx.position);

const swap = (ctx: PositionContext, ticks: PublicKey[]) => [
  meta(vaultSyrup(ctx), true), meta(POOL, true), meta(VAULT_A, true), meta(VAULT_B, true),
  meta(ticks[0], true), meta(ticks[1], true), meta(ticks[2], true),
  meta(ORACLE, false), meta(ORCA, false), meta(CHAINLINK, false),
];

export const mapleAdapter: AdapterDef = {
  label: "maple",
  programId: MAPLE_ADAPTER_PROGRAM_ID,
  baseMint: USDC_MINT,
  isInstant: true,
  computeUnits: 600_000,
  vaultTokenAccount: vaultUsdc,
  depositRemaining: (ctx) => swap(ctx, BUY_TICKS),
  valueRemaining: () => [meta(CHAINLINK, false)],
  withdrawRemaining: (ctx) => [meta(ctx.ticket, true), ...swap(ctx, SELL_TICKS)],
  async buildInitPosition(program, ctx) {
    return program.methods
      .initializePosition()
      .accountsPartial({
        position: ctx.position,
        vaultAuthority: ctx.vaultAuthority,
        baseMint: ctx.baseMint,
        syrupMint: SYRUP,
        vaultUsdc: vaultUsdc(ctx),
        vaultSyrup: vaultSyrup(ctx),
        owner: ctx.owner,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SYSTEM_PROGRAM_ID,
      })
      .instruction();
  },
};
