// Jupiter JLP adapter — ported from tests/adapters/jlp.spec.ts (verified working on fork).
// add/remove_liquidity2 revalue the whole pool, so the CPI carries the most accounts (base 11 +
// all 5 custodies + their 5 doves_ag oracles). value = NAV (jlp × aum_usd / supply).
// NOTE: the doves_ag 5s staleness window is a *fork-test* concern (refreshed via cheatcodes in the
// harness); on mainnet the oracles are live, so the SDK only sets the compute budget here.
// See docs/adapters/jupiter-jlp.md.
import * as anchor from "@anchor-lang/core";
import { PublicKey } from "@solana/web3.js";
import { AdapterDef, PositionContext, meta } from "./types";
import { subVaultPda } from "../pdas";
import { USDC_MINT, TOKEN_PROGRAM_ID, SYSTEM_PROGRAM_ID } from "../constants";

export const JLP_ADAPTER_PROGRAM_ID = new PublicKey("9fqh4833yoSJoPzpsucHY2SbUafVfHcC48RLQhTTahsB");

// Verified mainnet accounts.
const PERPS = new PublicKey("PERPHjGBqRHArX4DySjwM6UJHiR3sWAatqfdBS2qQJu");
const POOL = new PublicKey("5BUwFW4nRbftYTDMbgxykoFWqWHPzahFSNAaaaJtVKsq");
const JLP_MINT = new PublicKey("27G8MtK7VtTcCHkpASjSDdkWWYfoqT6ggEuKidVJidD4");
const PERPETUALS = new PublicKey("H4ND9aYttUVLFmNypZqLjZ52FYiGvdEB45GmwNoKEjTj");
const TRANSFER_AUTH = new PublicKey("AVzP2GeRmqGphJsMxWoqjpUifPpCret7LqWhD8NWQK49");
const EVENT_AUTH = new PublicKey("37hJBDnntwqhGbK7L6M1bLyvccj4u55CCUiLPdYkiqBN");
const USDC_CUSTODY = new PublicKey("G18jKKXQwBbrHeiK3C9MRXhkHsLHf7XgCSisykV46EZa");
const USDC_CUSTODY_TOKEN = new PublicKey("WzWUoCmtVv7eqAbU3BfKPU3fhLP6CXR8NCJH78UK9VS");
const USDC_DOVES_AG = new PublicKey("6Jp2xZUTWdDD2ZyUPRzeMdc6AFQ5K3pFgZxk2EijfjnM");
const CUSTODIES = [
  "7xS2gz2bTp3fwCC7knJvUWTEU9Tycczu6VhJYKgi1wdz", "AQCGyheWPLeo6Qp9WpYS9m3Qj479t7R636N9ey1rEjEn",
  "5Pv3gM9JrFFH883SWAhvJC9RPYmo8UNxuFtv5bMMALkm", "G18jKKXQwBbrHeiK3C9MRXhkHsLHf7XgCSisykV46EZa",
  "4vkNeXiYEUizLdrpdPS1eC2mccyM4NUPRtERrk6ZETkk",
].map((s) => new PublicKey(s));
const DOVES_AG = [
  "FYq2BWQ1V5P1WFBqr3qB2Kb5yHVvSv7upzKodgQE5zXh", "AFZnHPzy4mvVCffrVwhewHbFc93uTHvDSFrVH7GtfXF1",
  "hUqAT1KQ7eW1i6Csp9CXYtpPfSAvi835V7wKi5fRfmC", "6Jp2xZUTWdDD2ZyUPRzeMdc6AFQ5K3pFgZxk2EijfjnM",
  "Fgc93D641F8N2d1xLjQ4jmShuD3GE3BsCXA56KBQbF5u",
].map((s) => new PublicKey(s));

const vaultUsdc = (ctx: PositionContext) => subVaultPda(ctx.programId, "vault_usdc", ctx.position);
const vaultJlp = (ctx: PositionContext) => subVaultPda(ctx.programId, "vault_jlp", ctx.position);

// base 11 (vault_jlp..perps_program) + trailing 10 (5 custodies + 5 doves_ag), per adapter R-indices.
const protocol = (ctx: PositionContext) => [
  meta(vaultJlp(ctx), true), meta(TRANSFER_AUTH, false), meta(PERPETUALS, false), meta(POOL, true),
  meta(USDC_CUSTODY, true), meta(USDC_DOVES_AG, false), meta(USDC_DOVES_AG, false), meta(USDC_CUSTODY_TOKEN, true),
  meta(JLP_MINT, true), meta(EVENT_AUTH, false), meta(PERPS, false),
  ...CUSTODIES.map((c) => meta(c, false)), ...DOVES_AG.map((d) => meta(d, false)),
];

export const jlpAdapter: AdapterDef = {
  label: "jlp",
  programId: JLP_ADAPTER_PROGRAM_ID,
  baseMint: USDC_MINT,
  isInstant: true,
  computeUnits: 800_000,
  vaultTokenAccount: vaultUsdc,
  depositRemaining: protocol,
  withdrawRemaining: (ctx) => [meta(ctx.ticket, true), ...protocol(ctx)],
  valueRemaining: () => [meta(POOL, false), meta(JLP_MINT, false)],
  async buildInitPosition(program, ctx) {
    return program.methods
      .initializePosition()
      .accountsPartial({
        position: ctx.position,
        vaultAuthority: ctx.vaultAuthority,
        baseMint: ctx.baseMint,
        jlpMint: JLP_MINT,
        vaultUsdc: vaultUsdc(ctx),
        vaultJlp: vaultJlp(ctx),
        owner: ctx.owner,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SYSTEM_PROGRAM_ID,
      })
      .instruction();
  },
};
