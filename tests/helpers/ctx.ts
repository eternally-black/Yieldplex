// Shared test context: provider, program clients (from generated IDLs), PDA helpers.
import * as anchor from "@anchor-lang/core";
import { Connection, Keypair, PublicKey } from "@solana/web3.js";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";

export const RPC = process.env.ANCHOR_PROVIDER_URL || "http://127.0.0.1:8899";
const WALLET = process.env.ANCHOR_WALLET || path.join(os.homedir(), ".config/solana/id.json");

export function loadKeypair(p: string): Keypair {
  return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(fs.readFileSync(p, "utf8"))));
}

export const payer = loadKeypair(WALLET);
export const connection = new Connection(RPC, "confirmed");
export const provider = new anchor.AnchorProvider(connection, new anchor.Wallet(payer), {
  commitment: "confirmed",
  skipPreflight: false,
});
anchor.setProvider(provider);

const idlDir = path.join(__dirname, "..", "..", "target", "idl");
const idl = (name: string) => JSON.parse(fs.readFileSync(path.join(idlDir, `${name}.json`), "utf8"));

export const registry = new anchor.Program(idl("ya_registry"), provider);
export const dispatcher = new anchor.Program(idl("ya_dispatcher"), provider);
export const mock = new anchor.Program(idl("ya_mock_adapter"), provider);

export const SYSTEM_PROGRAM = anchor.web3.SystemProgram.programId;
export const TOKEN_PROGRAM = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
export const ASSOCIATED_TOKEN_PROGRAM = new PublicKey("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

/** Canonical associated token account for (owner, mint) on the SPL Token program. */
export const ata = (mint: PublicKey, owner: PublicKey) =>
  PublicKey.findProgramAddressSync(
    [owner.toBuffer(), TOKEN_PROGRAM.toBuffer(), mint.toBuffer()],
    ASSOCIATED_TOKEN_PROGRAM,
  )[0];

const S = {
  POSITION: Buffer.from("position"),
  VAULT_AUTHORITY: Buffer.from("vault_authority"),
  TICKET: Buffer.from("ticket"),
  REGISTRY: Buffer.from("registry"),
  ADAPTER: Buffer.from("adapter"),
};

export const registryPda = () =>
  PublicKey.findProgramAddressSync([S.REGISTRY], registry.programId)[0];
export const entryPda = (programId: PublicKey) =>
  PublicKey.findProgramAddressSync([S.ADAPTER, programId.toBuffer()], registry.programId)[0];
export const positionPda = (adapter: PublicKey, owner: PublicKey, baseMint: PublicKey) =>
  PublicKey.findProgramAddressSync([S.POSITION, owner.toBuffer(), baseMint.toBuffer()], adapter)[0];
export const vaultAuthorityPda = (adapter: PublicKey, position: PublicKey) =>
  PublicKey.findProgramAddressSync([S.VAULT_AUTHORITY, position.toBuffer()], adapter)[0];
export const ticketPda = (adapter: PublicKey, position: PublicKey) =>
  PublicKey.findProgramAddressSync([S.TICKET, position.toBuffer()], adapter)[0];

/** Idempotently initialize the registry (governance = guardian = our payer). */
export async function ensureRegistry(): Promise<void> {
  const reg = registryPda();
  if (await connection.getAccountInfo(reg)) return;
  await registry.methods
    .initializeRegistry(payer.publicKey, payer.publicKey)
    .accounts({ registry: reg, payer: payer.publicKey, systemProgram: SYSTEM_PROGRAM })
    .rpc();
}

/** Propose + approve an adapter (idempotent-ish: skips if already an Active entry). */
export async function ensureActiveAdapter(adapterProgram: PublicKey, baseMint: PublicKey, name: string): Promise<void> {
  const entry = entryPda(adapterProgram);
  const existing = await connection.getAccountInfo(entry);
  if (!existing) {
    await registry.methods
      .proposeAdapter(adapterProgram, baseMint, name, 1, 0, 9)
      .accounts({ registry: registryPda(), adapterEntry: entry, governance: payer.publicKey, systemProgram: SYSTEM_PROGRAM })
      .rpc();
    await registry.methods
      .approveAdapter()
      .accounts({ registry: registryPda(), adapterEntry: entry, governance: payer.publicKey })
      .rpc();
  }
}

export interface RouteAccountOverrides {
  /** Force a wrong base_mint (base-mint-mismatch gating test). */
  baseMintOverride?: PublicKey;
  /** Real vault token account (PDA) for token-moving adapters; defaults to a throwaway key. */
  vaultTokenAccount?: PublicKey;
  /** Real owner token account (funded ATA) for token-moving adapters; defaults to a throwaway key. */
  ownerTokenAccount?: PublicKey;
}

/** The standard 9-account prefix for a dispatcher route into `adapter`, for `owner`/`baseMint`. */
export function routeAccounts(
  adapter: PublicKey,
  owner: PublicKey,
  baseMint: PublicKey,
  opts: RouteAccountOverrides = {},
) {
  const position = positionPda(adapter, owner, baseMint);
  return {
    position,
    vaultAuthority: vaultAuthorityPda(adapter, position),
    baseMint: opts.baseMintOverride ?? baseMint,
    vaultTokenAccount: opts.vaultTokenAccount ?? Keypair.generate().publicKey,
    owner,
    ownerTokenAccount: opts.ownerTokenAccount ?? Keypair.generate().publicKey,
    registryEntry: entryPda(adapter),
    tokenProgram: TOKEN_PROGRAM,
    systemProgram: SYSTEM_PROGRAM,
    adapterProgram: adapter,
  };
}

/** Simulate a built tx and return the program returnData as a little-endian u64 (or null). */
export async function readReturnedU64(tx: anchor.web3.Transaction): Promise<bigint | null> {
  tx.feePayer = payer.publicKey;
  tx.recentBlockhash = (await connection.getLatestBlockhash()).blockhash;
  const sim = await connection.simulateTransaction(tx);
  const rd = sim.value.returnData;
  if (!rd) return null;
  const buf = Buffer.from(rd.data[0], "base64");
  return buf.length >= 8 ? buf.readBigUInt64LE(0) : null;
}
