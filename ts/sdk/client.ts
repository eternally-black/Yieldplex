// YieldAdapterClient — the integrator one-liner surface. Every call routes through the on-chain
// dispatcher (registry-gated, non-custodial) into the target adapter using the standard 9-account
// prefix + the adapter's protocol accounts, with a compute-budget ix injected automatically.
import * as anchor from "@anchor-lang/core";
import {
  ComputeBudgetProgram, Connection, PublicKey, Transaction, TransactionInstruction,
} from "@solana/web3.js";
import { AdapterDef, PositionContext } from "./adapters/types";
import { positionPda, vaultAuthorityPda, ticketPda, adapterEntryPda, ata } from "./pdas";
import { TOKEN_PROGRAM_ID, SYSTEM_PROGRAM_ID, REGISTRY_PROGRAM_ID } from "./constants";
import { decodePosition, decodeWithdrawalTicket, Position, WithdrawalTicket } from "./decode";
import { simulateReturnedU64 } from "./returnData";

export type Amount = anchor.BN | number | bigint;
const toBN = (x: Amount): anchor.BN => (x instanceof anchor.BN ? x : new anchor.BN(x.toString()));

export interface YieldAdapterClientOpts {
  provider: anchor.AnchorProvider;
  /** ya_dispatcher program client (from its IDL). */
  dispatcher: anchor.Program;
  /** The target adapter's program client (from its IDL) — used for initialize_position + its id. */
  adapter: anchor.Program;
  def: AdapterDef;
  /** Defaults to provider.wallet.publicKey. */
  owner?: PublicKey;
  /** Defaults to REGISTRY_PROGRAM_ID. */
  registryProgramId?: PublicKey;
}

export class YieldAdapterClient {
  readonly provider: anchor.AnchorProvider;
  readonly connection: Connection;
  readonly dispatcher: anchor.Program;
  readonly adapter: anchor.Program;
  readonly def: AdapterDef;
  readonly owner: PublicKey;
  readonly programId: PublicKey;
  readonly registryProgramId: PublicKey;

  constructor(opts: YieldAdapterClientOpts) {
    this.provider = opts.provider;
    this.connection = opts.provider.connection;
    this.dispatcher = opts.dispatcher;
    this.adapter = opts.adapter;
    this.def = opts.def;
    this.owner = opts.owner ?? opts.provider.wallet.publicKey;
    this.programId = opts.adapter.programId;
    this.registryProgramId = opts.registryProgramId ?? REGISTRY_PROGRAM_ID;
  }

  // ── PDAs ──────────────────────────────────────────────────
  get position(): PublicKey { return positionPda(this.programId, this.owner, this.def.baseMint); }
  get vaultAuthority(): PublicKey { return vaultAuthorityPda(this.programId, this.position); }
  get ticket(): PublicKey { return ticketPda(this.programId, this.position); }
  get registryEntry(): PublicKey { return adapterEntryPda(this.registryProgramId, this.programId); }

  private ctx(): PositionContext {
    const position = this.position;
    return {
      programId: this.programId,
      owner: this.owner,
      baseMint: this.def.baseMint,
      position,
      vaultAuthority: vaultAuthorityPda(this.programId, position),
      ticket: ticketPda(this.programId, position),
    };
  }

  /** The standard 9-account prefix the dispatcher's Route struct expects. */
  private routePrefix() {
    const ctx = this.ctx();
    return {
      position: ctx.position,
      vaultAuthority: ctx.vaultAuthority,
      baseMint: ctx.baseMint,
      // non-token adapters ignore the token accounts; default to derivable, harmless keys.
      vaultTokenAccount: this.def.vaultTokenAccount?.(ctx) ?? ctx.vaultAuthority,
      owner: this.owner,
      ownerTokenAccount: this.def.ownerTokenAccount?.(ctx) ?? ata(this.def.baseMint, this.owner),
      registryEntry: this.registryEntry,
      tokenProgram: TOKEN_PROGRAM_ID,
      systemProgram: SYSTEM_PROGRAM_ID,
      adapterProgram: this.programId,
    };
  }

  private cuIxs(): TransactionInstruction[] {
    return this.def.computeUnits
      ? [ComputeBudgetProgram.setComputeUnitLimit({ units: this.def.computeUnits })]
      : [];
  }

  // ── lifecycle ─────────────────────────────────────────────
  /** Create the position (and its protocol sub-accounts). Idempotent per adapter. */
  async initPosition(): Promise<string> {
    const ix = await this.def.buildInitPosition(this.adapter, this.ctx());
    const tx = new Transaction().add(...this.cuIxs(), ix);
    return this.provider.sendAndConfirm(tx, []);
  }

  /** Deposit `amount` base units; reverts unless the position grows by ≥ `minPositionOut`. */
  async deposit(amount: Amount, minPositionOut: Amount = 0, extraPre: TransactionInstruction[] = []): Promise<string> {
    return this.dispatcher.methods
      .routeDeposit(toBN(amount), toBN(minPositionOut))
      .accountsPartial(this.routePrefix())
      .remainingAccounts(this.def.depositRemaining(this.ctx()))
      .preInstructions([...this.cuIxs(), ...extraPre])
      .rpc();
  }

  /** Withdraw `shares`. Instant adapters settle here; two-phase open a Pending ticket. */
  async withdraw(shares: Amount, minAmountOut: Amount = 0, extraPre: TransactionInstruction[] = []): Promise<string> {
    return this.dispatcher.methods
      .routeWithdraw(toBN(shares), toBN(minAmountOut))
      .accountsPartial(this.routePrefix())
      .remainingAccounts(this.def.withdrawRemaining(this.ctx()))
      .preInstructions([...this.cuIxs(), ...extraPre])
      .rpc();
  }

  /** Settle a Pending two-phase withdrawal after its cooldown unlock. */
  async settle(minAmountOut: Amount = 0, extraPre: TransactionInstruction[] = []): Promise<string> {
    return this.dispatcher.methods
      .routeSettleWithdrawal(toBN(minAmountOut))
      .accountsPartial(this.routePrefix())
      .remainingAccounts(this.def.withdrawRemaining(this.ctx()))
      .preInstructions([...this.cuIxs(), ...extraPre])
      .rpc();
  }

  /** View: the position's current redeemable base-asset value (u64), via the dispatcher view. */
  async currentValue(extraPre: TransactionInstruction[] = []): Promise<bigint | null> {
    const tx: Transaction = await this.dispatcher.methods
      .routeCurrentValue()
      .accountsPartial(this.routePrefix())
      .remainingAccounts(this.def.valueRemaining(this.ctx()))
      .preInstructions([...this.cuIxs(), ...extraPre])
      .transaction();
    return simulateReturnedU64(this.connection, tx, this.owner);
  }

  // ── reads (the single decoder) ────────────────────────────
  async fetchPosition(): Promise<Position | null> {
    const ai = await this.connection.getAccountInfo(this.position);
    return ai ? decodePosition(ai.data) : null;
  }

  async fetchTicket(): Promise<WithdrawalTicket | null> {
    const ai = await this.connection.getAccountInfo(this.ticket);
    return ai ? decodeWithdrawalTicket(ai.data) : null;
  }

  /** Poll the ticket until its unlock_ts passes, then settle (two-phase adapters). */
  async waitAndSettle(opts: { pollMs?: number; timeoutMs?: number; minAmountOut?: Amount } = {}): Promise<string> {
    const pollMs = opts.pollMs ?? 15_000;
    const deadline = nowMs() + (opts.timeoutMs ?? 30 * 24 * 3600 * 1000);
    for (;;) {
      const t = await this.fetchTicket();
      if (!t || t.status !== "pending") throw new Error("no pending withdrawal ticket to settle");
      if (Number(t.unlockTs) <= Math.floor(nowMs() / 1000)) break;
      if (nowMs() > deadline) throw new Error("timed out waiting for withdrawal unlock");
      await sleep(pollMs);
    }
    return this.settle(opts.minAmountOut ?? 0);
  }
}

function nowMs(): number { return Date.now(); }
function sleep(ms: number): Promise<void> { return new Promise((r) => setTimeout(r, ms)); }
