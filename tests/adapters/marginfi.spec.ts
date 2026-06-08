// MarginFi v2 USDC adapter — mainnet-fork conformance + EDGE value-accuracy (diff=0).
// Mirrors kamino.spec.ts against real cloned MarginFi state.
import * as anchor from "@anchor-lang/core";
import { AccountMeta, ComputeBudgetProgram, PublicKey } from "@solana/web3.js";
import { assert } from "chai";
import * as fs from "fs";
import * as path from "path";
import {
  payer, provider, connection, dispatcher, routeAccounts, readReturnedU64,
  positionPda, vaultAuthorityPda, ticketPda, ensureRegistry, ensureActiveAdapter,
  SYSTEM_PROGRAM, TOKEN_PROGRAM, ata,
} from "../helpers/ctx";
import { runConformance } from "../conformance/runConformance";
import { fundUsdc, cheat } from "../helpers/cheatcodes";

// Verified on-chain (M0 + scripts/inspect-marginfi.mjs).
const USDC = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
const MARGINFI = new PublicKey("MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA");
const GROUP = new PublicKey("4qp6Fx6tnZkY5Wropq9wUYgtFxXKwE6viZxFHg3rdAG8");
const BANK = new PublicKey("2s37akK2eyBbp8DZgCm7RtsaEz8eJP3Nxd4urLHQv7yB");
const LIQ_VAULT = new PublicKey("7jaiZR5Sk8hdYN9MxTpczTcwbWpb5WEoxSANuUwveuat");
const ORACLE = new PublicKey("Dpw1EAVrSB1ibxiDQyTAW6Zip3J4Btk2x4SgApQCeFbX");
const LIQ_VAULT_AUTH = PublicKey.findProgramAddressSync(
  [Buffer.from("liquidity_vault_auth"), BANK.toBuffer()], MARGINFI)[0];

const idlDir = path.join(__dirname, "..", "..", "target", "idl");
const marginfi = new anchor.Program(
  JSON.parse(fs.readFileSync(path.join(idlDir, "ya_adapter_marginfi.json"), "utf8")),
  provider,
);

describe("ya-adapter-marginfi — conformance + value accuracy (surfnet, real MarginFi state)", () => {
  const owner = payer.publicKey;
  const adapter = marginfi.programId;
  const position = positionPda(adapter, owner, USDC);
  const vaultAuthority = vaultAuthorityPda(adapter, position);
  const vaultUsdc = PublicKey.findProgramAddressSync(
    [Buffer.from("vault_usdc"), position.toBuffer()], adapter)[0];
  const marginfiAccount = PublicKey.findProgramAddressSync(
    [Buffer.from("marginfi_account"), position.toBuffer()], adapter)[0];
  const ownerUsdc = ata(USDC, owner);

  const depositMetas = (): AccountMeta[] => [
    { pubkey: marginfiAccount, isSigner: false, isWritable: true },
    { pubkey: GROUP, isSigner: false, isWritable: false },
    { pubkey: BANK, isSigner: false, isWritable: true },
    { pubkey: LIQ_VAULT, isSigner: false, isWritable: true },
    { pubkey: MARGINFI, isSigner: false, isWritable: false },
  ];
  const valueMetas = (): AccountMeta[] => [
    { pubkey: marginfiAccount, isSigner: false, isWritable: false },
    { pubkey: BANK, isSigner: false, isWritable: false },
  ];
  const withdrawProtocol = (): AccountMeta[] => [
    { pubkey: marginfiAccount, isSigner: false, isWritable: true },
    { pubkey: GROUP, isSigner: false, isWritable: false },
    { pubkey: BANK, isSigner: false, isWritable: true },
    { pubkey: LIQ_VAULT_AUTH, isSigner: false, isWritable: true },
    { pubkey: LIQ_VAULT, isSigner: false, isWritable: true },
    { pubkey: ORACLE, isSigner: false, isWritable: false },
    { pubkey: MARGINFI, isSigner: false, isWritable: false },
  ];
  const cuPre = async () => [ComputeBudgetProgram.setComputeUnitLimit({ units: 800_000 })];
  const acc = () =>
    routeAccounts(adapter, owner, USDC, { vaultTokenAccount: vaultUsdc, ownerTokenAccount: ownerUsdc });

  before(async () => {
    await ensureRegistry();
    await ensureActiveAdapter(adapter, USDC, "marginfi");
    await fundUsdc(owner, 1_000_000_000n); // 1,000 USDC
  });

  runConformance(() => ({
    label: "marginfi",
    adapter: marginfi,
    baseMint: USDC,
    depositAmount: new anchor.BN(25_000_000),
    toleranceBps: 1, // value == deposit within share floor rounding
    isInstant: true,
    vaultTokenAccount: () => vaultUsdc,
    ownerTokenAccount: () => ownerUsdc,
    depositRemaining: depositMetas,
    valueRemaining: valueMetas,
    withdrawRemaining: () => [
      { pubkey: ticketPda(adapter, position), isSigner: false, isWritable: true },
      ...withdrawProtocol(),
    ],
    preInstructions: cuPre,
    initPosition: async () => {
      await marginfi.methods
        .initializePosition()
        .accountsPartial({
          position,
          vaultAuthority,
          baseMint: USDC,
          vaultUsdc,
          marginfiAccount,
          marginfiGroup: GROUP,
          marginfiProgram: MARGINFI,
          owner,
          tokenProgram: TOKEN_PROGRAM,
          systemProgram: SYSTEM_PROGRAM,
        })
        .preInstructions(await cuPre())
        .rpc();
    },
  }));

  it("EDGE: current_value() == actual redeemed USDC, diff = 0 lamports", async () => {
    const BN0 = new anchor.BN(0);
    // MarginFi accrues interest lazily (only on deposit/withdraw). Pin the surfnet clock so the
    // deposit and the withdraw accrue to the SAME instant — otherwise the withdraw earns a sliver
    // more interest than the committed state current_value reads (current_value stays conservative,
    // never overstating). This makes the diff=0 assertion a true same-instant comparison.
    // (Fork-only fixture — see tests/fork/FIXTURES.md; no production path depends on it.)
    // surfnet_timeTravel takes an absolute timestamp in MILLISECONDS and refuses to go backward.
    // Pin the deposit and withdraw to two instants within the SAME unix second (marginfi accrues
    // interest per second), strictly increasing in ms so neither is "past".
    const baseSec = ((await connection.getBlockTime(await connection.getSlot())) ?? 0) + 5;
    const pinMs = (ms: number) => cheat("surfnet_timeTravel", [{ absoluteTimestamp: ms }]);

    await pinMs(baseSec * 1000 + 1);
    await dispatcher.methods
      .routeDeposit(new anchor.BN(20_000_000), BN0)
      .accountsPartial(acc())
      .remainingAccounts(depositMetas())
      .preInstructions(await cuPre())
      .rpc();

    const tx = await dispatcher.methods
      .routeCurrentValue()
      .accountsPartial(acc())
      .remainingAccounts(valueMetas())
      .preInstructions(await cuPre())
      .transaction();
    const value = await readReturnedU64(tx);
    assert.isNotNull(value, "current_value must return data");

    const pos: any = await (marginfi.account as any).position.fetch(position);
    const shares = new anchor.BN(pos.shares.toString());
    const before = BigInt((await connection.getTokenAccountBalance(ownerUsdc)).value.amount);
    await pinMs(baseSec * 1000 + 900); // same second as the deposit -> identical interest accrual
    await dispatcher.methods
      .routeWithdraw(shares, BN0)
      .accountsPartial(acc())
      .remainingAccounts([
        { pubkey: ticketPda(adapter, position), isSigner: false, isWritable: true },
        ...withdrawProtocol(),
      ])
      .preInstructions(await cuPre())
      .rpc();
    const after = BigInt((await connection.getTokenAccountBalance(ownerUsdc)).value.amount);
    const redeemed = after - before;

    console.log(`    [marginfi] current_value=${value}  redeemed=${redeemed}  diff=${(value as bigint) - redeemed}`);
    assert.equal((value as bigint).toString(), redeemed.toString(), "current_value must equal redeemed (diff=0)");
  });
});
