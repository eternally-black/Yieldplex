// M4 conformance proof: the parametrized suite run against the deployed mock adapter on surfnet.
// Real adapters (M5) reuse `runConformance` the same way, supplying protocol-specific builders.
import * as anchor from "@anchor-lang/core";
import { PublicKey } from "@solana/web3.js";
import {
  payer, mock, positionPda, vaultAuthorityPda, ticketPda,
  ensureRegistry, ensureActiveAdapter, SYSTEM_PROGRAM,
} from "./helpers/ctx";
import { runConformance } from "./conformance/runConformance";

describe("ya-mock-adapter — conformance suite (surfnet)", () => {
  const baseMint = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
  const adapter = mock.programId;
  const owner = payer.publicKey;

  before(async () => {
    await ensureRegistry();
    await ensureActiveAdapter(adapter, baseMint, "mock");
  });

  runConformance(() => ({
    label: "mock",
    adapter: mock,
    baseMint,
    depositAmount: new anchor.BN(25_000_000),
    toleranceBps: 0, // mock is exact 1:1
    isInstant: true,
    initPosition: async () => {
      const position = positionPda(adapter, owner, baseMint);
      await mock.methods
        .initializePosition()
        .accountsPartial({
          position,
          vaultAuthority: vaultAuthorityPda(adapter, position),
          baseMint,
          owner,
          systemProgram: SYSTEM_PROGRAM,
        })
        .rpc();
    },
    withdrawRemaining: () => {
      const position = positionPda(adapter, owner, baseMint);
      return [{ pubkey: ticketPda(adapter, position), isSigner: false, isWritable: true }];
    },
  }));
});
