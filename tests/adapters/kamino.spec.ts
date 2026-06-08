// Kamino Lend USDC adapter — mainnet-fork conformance + the EDGE value-accuracy proof.
// Runs the shared runConformance suite against the deployed adapter on Surfpool (real cloned
// Kamino state), then asserts current_value() == the ACTUAL redeemed USDC to the lamport (diff=0).
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
import { fundUsdc } from "../helpers/cheatcodes";

// Verified on-chain (M0 + scripts/inspect-kamino.ts).
const USDC = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
const KAMINO = new PublicKey("KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD");
const MARKET = new PublicKey("7u3HeHxYDLhnCoErrtycNokbQYbWGzLs6JSDqGAv5PfF");
const RESERVE = new PublicKey("D6q6wuQSrifJKZYpR1M8R4YawnLDtDsMmWM1NbBmgJ59");
const LMA = new PublicKey("9DrvZvyWh1HuAoZxvYWMvkf2XCzryCpGgHqrMjyDWpmo");
const LIQ_SUPPLY = new PublicKey("Bgq7trRgVMeq33yt235zM2onQ4bRDBsY5EWiTetF4qw6");
const COLL_MINT = new PublicKey("B8V6WVjPxW1UGwVDfxH2d2r8SyT4cqn7dQRK6XneVa7D");
const INSTR_SYSVAR = new PublicKey("Sysvar1nstructions1111111111111111111111111");

const idlDir = path.join(__dirname, "..", "..", "target", "idl");
const kamino = new anchor.Program(
  JSON.parse(fs.readFileSync(path.join(idlDir, "ya_adapter_kamino.json"), "utf8")),
  provider,
);

describe("ya-adapter-kamino — conformance + value accuracy (surfnet, real Kamino state)", () => {
  const owner = payer.publicKey;
  const adapter = kamino.programId;
  const position = positionPda(adapter, owner, USDC);
  const vaultAuthority = vaultAuthorityPda(adapter, position);
  const vaultUsdc = PublicKey.findProgramAddressSync(
    [Buffer.from("vault_usdc"), position.toBuffer()], adapter)[0];
  const vaultCtoken = PublicKey.findProgramAddressSync(
    [Buffer.from("vault_ctoken"), position.toBuffer()], adapter)[0];
  const ownerUsdc = ata(USDC, owner);

  // Kamino accounts after the standard 9-prefix (order matches the adapter's R_* indices).
  const protocolMetas = (): AccountMeta[] => [
    { pubkey: vaultCtoken, isSigner: false, isWritable: true },
    { pubkey: RESERVE, isSigner: false, isWritable: true },
    { pubkey: MARKET, isSigner: false, isWritable: false },
    { pubkey: LMA, isSigner: false, isWritable: false },
    { pubkey: LIQ_SUPPLY, isSigner: false, isWritable: true },
    { pubkey: COLL_MINT, isSigner: false, isWritable: true },
    { pubkey: INSTR_SYSVAR, isSigner: false, isWritable: false },
    { pubkey: KAMINO, isSigner: false, isWritable: false },
  ];
  const reserveMeta = (): AccountMeta[] => [{ pubkey: RESERVE, isSigner: false, isWritable: false }];
  const cuPre = async () => [ComputeBudgetProgram.setComputeUnitLimit({ units: 600_000 })];
  const acc = () =>
    routeAccounts(adapter, owner, USDC, { vaultTokenAccount: vaultUsdc, ownerTokenAccount: ownerUsdc });

  before(async () => {
    await ensureRegistry();
    await ensureActiveAdapter(adapter, USDC, "kamino");
    await fundUsdc(owner, 1_000_000_000n); // 1,000 USDC
  });

  it("lending_market_authority seed [b\"lma\", market] derives to the on-chain authority", () => {
    const [lma] = PublicKey.findProgramAddressSync([Buffer.from("lma"), MARKET.toBuffer()], KAMINO);
    assert.equal(lma.toBase58(), LMA.toBase58());
  });

  runConformance(() => ({
    label: "kamino",
    adapter: kamino,
    baseMint: USDC,
    depositAmount: new anchor.BN(25_000_000), // 25 USDC
    toleranceBps: 1, // value == deposit within cToken floor rounding (~1 unit)
    isInstant: true,
    vaultTokenAccount: () => vaultUsdc,
    ownerTokenAccount: () => ownerUsdc,
    depositRemaining: protocolMetas,
    valueRemaining: reserveMeta,
    withdrawRemaining: () => [
      { pubkey: ticketPda(adapter, position), isSigner: false, isWritable: true },
      ...protocolMetas(),
    ],
    preInstructions: cuPre,
    initPosition: async () => {
      await kamino.methods
        .initializePosition()
        .accountsPartial({
          position,
          vaultAuthority,
          baseMint: USDC,
          collateralMint: COLL_MINT,
          vaultUsdc,
          vaultCtoken,
          owner,
          tokenProgram: TOKEN_PROGRAM,
          systemProgram: SYSTEM_PROGRAM,
        })
        .rpc();
    },
  }));

  it("EDGE: current_value() == actual redeemed USDC, diff = 0 lamports", async () => {
    const BN0 = new anchor.BN(0);
    // Fresh deposit (the conformance withdraw test emptied the position).
    await dispatcher.methods
      .routeDeposit(new anchor.BN(20_000_000), BN0)
      .accountsPartial(acc())
      .remainingAccounts(protocolMetas())
      .preInstructions(await cuPre())
      .rpc();

    // On-chain current_value (via the dispatcher view).
    const tx = await dispatcher.methods
      .routeCurrentValue()
      .accountsPartial(acc())
      .remainingAccounts(reserveMeta())
      .preInstructions(await cuPre())
      .transaction();
    const value = await readReturnedU64(tx);
    assert.isNotNull(value, "current_value must return data");

    // Redeem all and measure the actual USDC delta on the owner's ATA.
    const pos: any = await (kamino.account as any).position.fetch(position);
    const shares = new anchor.BN(pos.shares.toString());
    const before = BigInt((await connection.getTokenAccountBalance(ownerUsdc)).value.amount);
    await dispatcher.methods
      .routeWithdraw(shares, BN0)
      .accountsPartial(acc())
      .remainingAccounts([
        { pubkey: ticketPda(adapter, position), isSigner: false, isWritable: true },
        ...protocolMetas(),
      ])
      .preInstructions(await cuPre())
      .rpc();
    const after = BigInt((await connection.getTokenAccountBalance(ownerUsdc)).value.amount);
    const redeemed = after - before;

    const diff = (value as bigint) - redeemed;
    console.log(`    [kamino] current_value=${value}  redeemed=${redeemed}  diff=${diff}`);
    assert.equal(
      (value as bigint).toString(),
      redeemed.toString(),
      "current_value must equal the actual redeemed USDC (diff = 0)",
    );
  });
});
