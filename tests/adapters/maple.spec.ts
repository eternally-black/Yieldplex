// Maple syrupUSDC adapter — mainnet-fork conformance + value-accuracy.
// Swap-and-hold via one Orca Whirlpool swap; value via the Chainlink syrupUSDC exchange rate.
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

const USDC = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
const ORCA = new PublicKey("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc");
const POOL = new PublicKey("6fteKNvMdv7tYmBoJHhj1jx6rHcEwC6RdSEmVpyS613J");
const SYRUP = new PublicKey("AvZZF1YaZDziPY2RCK4oJrRVrbN3mTD9NL24hPeaZeUj");
const VAULT_A = new PublicKey("FM2RuqFYo9umA1yc5FyQn6pSDZJZ1MXAdaekJZ4dQCvi"); // syrup vault (A)
const VAULT_B = new PublicKey("Fw6Xr45rBBrXbWJd5ZbSg44kacrKRLef4rHkZ8gWC5Ab"); // USDC vault (B)
const ORACLE = new PublicKey("H7j5FQpwTUMwxrWeuyrLr5Z9oHsPFiaRqNaERVsuE1c8"); // ["oracle", whirlpool]
const CHAINLINK = new PublicKey("CpNyiFt84q66665Kx64bobxZuMgZ2EecrhAJs1HikS2T");
// tick arrays differ by direction (deposit buys syrup = ascending ticks; withdraw sells = descending)
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

const idlDir = path.join(__dirname, "..", "..", "target", "idl");
const maple = new anchor.Program(
  JSON.parse(fs.readFileSync(path.join(idlDir, "ya_adapter_maple.json"), "utf8")),
  provider,
);

describe("ya-adapter-maple — conformance + value accuracy (surfnet, real Orca/Chainlink state)", () => {
  const owner = payer.publicKey;
  const adapter = maple.programId;
  const position = positionPda(adapter, owner, USDC);
  const vaultAuthority = vaultAuthorityPda(adapter, position);
  const vaultUsdc = PublicKey.findProgramAddressSync([Buffer.from("vault_usdc"), position.toBuffer()], adapter)[0];
  const vaultSyrup = PublicKey.findProgramAddressSync([Buffer.from("vault_syrup"), position.toBuffer()], adapter)[0];
  const ownerUsdc = ata(USDC, owner);
  const m = (k: PublicKey, w: boolean): AccountMeta => ({ pubkey: k, isSigner: false, isWritable: w });
  const swapMetas = (ticks: PublicKey[]): AccountMeta[] => [
    m(vaultSyrup, true), m(POOL, true), m(VAULT_A, true), m(VAULT_B, true),
    m(ticks[0], true), m(ticks[1], true), m(ticks[2], true), m(ORACLE, false), m(ORCA, false), m(CHAINLINK, false),
  ];
  const cuPre = async () => [ComputeBudgetProgram.setComputeUnitLimit({ units: 600_000 })];
  const acc = () => routeAccounts(adapter, owner, USDC, { vaultTokenAccount: vaultUsdc, ownerTokenAccount: ownerUsdc });

  before(async () => {
    await ensureRegistry();
    await ensureActiveAdapter(adapter, USDC, "maple");
    await fundUsdc(owner, 1_000_000_000n);
  });

  runConformance(() => ({
    label: "maple",
    adapter: maple,
    baseMint: USDC,
    depositAmount: new anchor.BN(25_000_000),
    toleranceBps: 50, // swap fee (1bp) + price impact + chainlink-vs-pool spread
    isInstant: true,
    vaultTokenAccount: () => vaultUsdc,
    ownerTokenAccount: () => ownerUsdc,
    depositRemaining: () => swapMetas(BUY_TICKS),
    valueRemaining: () => [m(CHAINLINK, false)],
    withdrawRemaining: () => [m(ticketPda(adapter, position), true), ...swapMetas(SELL_TICKS)],
    preInstructions: cuPre,
    initPosition: async () => {
      await maple.methods
        .initializePosition()
        .accountsPartial({
          position, vaultAuthority, baseMint: USDC, syrupMint: SYRUP, vaultUsdc, vaultSyrup,
          owner, tokenProgram: TOKEN_PROGRAM, systemProgram: SYSTEM_PROGRAM,
        })
        .rpc();
    },
  }));

  it("EDGE: current_value() == syrupUSDC balance × Chainlink rate (diff = 0)", async () => {
    // Fresh deposit so the position holds a real syrupUSDC balance (conformance withdrew all).
    await dispatcher.methods
      .routeDeposit(new anchor.BN(20_000_000), new anchor.BN(0))
      .accountsPartial(acc())
      .remainingAccounts(swapMetas(BUY_TICKS))
      .preInstructions(await cuPre())
      .rpc();
    // current_value (on-chain) must equal our independent off-chain Chainlink computation.
    const tx = await dispatcher.methods
      .routeCurrentValue()
      .accountsPartial(acc())
      .remainingAccounts([m(CHAINLINK, false)])
      .preInstructions(await cuPre())
      .transaction();
    const value = await readReturnedU64(tx);
    assert.isNotNull(value, "current_value must return data");

    const syrupBal = BigInt((await connection.getTokenAccountBalance(vaultSyrup)).value.amount);
    const feed = (await connection.getAccountInfo(CHAINLINK))!.data;
    const decimals = feed[138];
    let answer = 0n;
    for (let i = 231; i >= 216; i--) answer = (answer << 8n) | BigInt(feed[i]); // i128 LE @216
    const expected = (syrupBal * answer) / 10n ** BigInt(decimals);

    console.log(`    [maple] syrup=${syrupBal} rate=${answer}(${decimals}dp) current_value=${value} expected=${expected}`);
    assert.equal((value as bigint).toString(), expected.toString(), "current_value must equal syrup × Chainlink rate");
  });
});
