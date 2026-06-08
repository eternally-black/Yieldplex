// Kamino Lend USDC adapter — ported from tests/adapters/kamino.spec.ts (verified working on fork).
// Standalone reserve path (deposit_reserve_liquidity / redeem_reserve_collateral); self-refreshing,
// no oracle. value = oracle-free cToken exchange rate. See docs/adapters/kamino.md.
import * as anchor from "@anchor-lang/core";
import { PublicKey } from "@solana/web3.js";
import { AdapterDef, PositionContext, meta } from "./types";
import { subVaultPda } from "../pdas";
import { USDC_MINT, TOKEN_PROGRAM_ID, SYSTEM_PROGRAM_ID } from "../constants";

export const KAMINO_ADAPTER_PROGRAM_ID = new PublicKey("BwyrWhHa86dCyRghZn9EDK2ZxfhpBH4tr5NVoBJ3hTs5");

// Verified mainnet accounts (main-market USDC reserve).
const KAMINO = new PublicKey("KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD");
const MARKET = new PublicKey("7u3HeHxYDLhnCoErrtycNokbQYbWGzLs6JSDqGAv5PfF");
const RESERVE = new PublicKey("D6q6wuQSrifJKZYpR1M8R4YawnLDtDsMmWM1NbBmgJ59");
const LMA = new PublicKey("9DrvZvyWh1HuAoZxvYWMvkf2XCzryCpGgHqrMjyDWpmo"); // [b"lma", market]
const LIQ_SUPPLY = new PublicKey("Bgq7trRgVMeq33yt235zM2onQ4bRDBsY5EWiTetF4qw6");
const COLL_MINT = new PublicKey("B8V6WVjPxW1UGwVDfxH2d2r8SyT4cqn7dQRK6XneVa7D");
const INSTR_SYSVAR = new PublicKey("Sysvar1nstructions1111111111111111111111111");

const vaultUsdc = (ctx: PositionContext) => subVaultPda(ctx.programId, "vault_usdc", ctx.position);
const vaultCtoken = (ctx: PositionContext) => subVaultPda(ctx.programId, "vault_ctoken", ctx.position);

const protocol = (ctx: PositionContext) => [
  meta(vaultCtoken(ctx), true),
  meta(RESERVE, true),
  meta(MARKET, false),
  meta(LMA, false),
  meta(LIQ_SUPPLY, true),
  meta(COLL_MINT, true),
  meta(INSTR_SYSVAR, false),
  meta(KAMINO, false),
];

export const kaminoAdapter: AdapterDef = {
  label: "kamino",
  programId: KAMINO_ADAPTER_PROGRAM_ID,
  baseMint: USDC_MINT,
  isInstant: true,
  computeUnits: 600_000,
  vaultTokenAccount: vaultUsdc,
  depositRemaining: protocol,
  withdrawRemaining: (ctx) => [meta(ctx.ticket, true), ...protocol(ctx)],
  valueRemaining: () => [meta(RESERVE, false)],
  async buildInitPosition(program, ctx) {
    return program.methods
      .initializePosition()
      .accountsPartial({
        position: ctx.position,
        vaultAuthority: ctx.vaultAuthority,
        baseMint: ctx.baseMint,
        collateralMint: COLL_MINT,
        vaultUsdc: vaultUsdc(ctx),
        vaultCtoken: vaultCtoken(ctx),
        owner: ctx.owner,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SYSTEM_PROGRAM_ID,
      })
      .instruction();
  },
};
