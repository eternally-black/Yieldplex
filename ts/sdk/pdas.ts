// Canonical PDA derivations for the Yield Adapter Standard. Seeds match the on-chain crate.
import { PublicKey } from "@solana/web3.js";
import {
  SEED_POSITION, SEED_VAULT_AUTHORITY, SEED_TICKET, SEED_REGISTRY, SEED_ADAPTER,
  TOKEN_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID,
} from "./constants";

/** `[b"position", owner, base_mint]` on the adapter program. */
export const positionPda = (adapter: PublicKey, owner: PublicKey, baseMint: PublicKey): PublicKey =>
  PublicKey.findProgramAddressSync([SEED_POSITION, owner.toBuffer(), baseMint.toBuffer()], adapter)[0];

/** `[b"vault_authority", position]` — signs every protocol CPI, owns the vault sub-accounts. */
export const vaultAuthorityPda = (adapter: PublicKey, position: PublicKey): PublicKey =>
  PublicKey.findProgramAddressSync([SEED_VAULT_AUTHORITY, position.toBuffer()], adapter)[0];

/** `[b"ticket", position]` — the single active withdrawal ticket per position. */
export const ticketPda = (adapter: PublicKey, position: PublicKey): PublicKey =>
  PublicKey.findProgramAddressSync([SEED_TICKET, position.toBuffer()], adapter)[0];

/** `[b"registry"]` singleton on the registry program. */
export const registryPda = (registryProgram: PublicKey): PublicKey =>
  PublicKey.findProgramAddressSync([SEED_REGISTRY], registryProgram)[0];

/** `[b"adapter", adapter_program]` entry on the registry program. */
export const adapterEntryPda = (registryProgram: PublicKey, adapter: PublicKey): PublicKey =>
  PublicKey.findProgramAddressSync([SEED_ADAPTER, adapter.toBuffer()], registryProgram)[0];

/** Generic per-position vault sub-account PDA: `[seed, position]` (e.g. vault_usdc / vault_ctoken). */
export const subVaultPda = (adapter: PublicKey, seed: string, position: PublicKey): PublicKey =>
  PublicKey.findProgramAddressSync([Buffer.from(seed), position.toBuffer()], adapter)[0];

/** Associated token account for (owner, mint) on the SPL Token program. */
export const ata = (mint: PublicKey, owner: PublicKey): PublicKey =>
  PublicKey.findProgramAddressSync(
    [owner.toBuffer(), TOKEN_PROGRAM_ID.toBuffer(), mint.toBuffer()],
    ASSOCIATED_TOKEN_PROGRAM_ID,
  )[0];
