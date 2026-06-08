// Offline unit test for the single decoder (no validator needed). Builds byte-exact Anchor/Borsh
// buffers for Position / WithdrawalTicket / AdapterEntry and round-trips them through the SDK.
// Run standalone: npx ts-mocha -p ./tsconfig.json tests/sdk/decode.spec.ts
import { Keypair } from "@solana/web3.js";
import { assert } from "chai";
import {
  decodePosition, decodeWithdrawalTicket, decodeAdapterEntry,
  POSITION_DISCRIMINATOR, WITHDRAWAL_TICKET_DISCRIMINATOR, ADAPTER_ENTRY_DISCRIMINATOR,
  POSITION_SIZE, WITHDRAWAL_TICKET_SIZE, ADAPTER_ENTRY_SIZE,
} from "../../ts/sdk/decode";

describe("SDK single decoder — byte-exact layout (offline)", () => {
  it("decodes a Position", () => {
    const owner = Keypair.generate().publicKey;
    const baseMint = Keypair.generate().publicKey;
    const adapter = Keypair.generate().publicKey;
    const buf = Buffer.alloc(POSITION_SIZE);
    POSITION_DISCRIMINATOR.copy(buf, 0);
    owner.toBuffer().copy(buf, 8);
    baseMint.toBuffer().copy(buf, 40);
    adapter.toBuffer().copy(buf, 72);
    buf.writeBigUInt64LE(123_456_789n, 104); // shares
    buf.writeBigUInt64LE(25_000_001n, 112); // cached_value
    buf.writeBigInt64LE(1_700_000_000n, 120); // value_updated_ts
    buf.writeUInt8(254, 128); // bump
    buf.writeUInt8(253, 129); // vault_authority_bump

    const p = decodePosition(buf);
    assert.equal(p.owner.toBase58(), owner.toBase58());
    assert.equal(p.baseMint.toBase58(), baseMint.toBase58());
    assert.equal(p.adapter.toBase58(), adapter.toBase58());
    assert.equal(p.shares.toString(), "123456789");
    assert.equal(p.cachedValue.toString(), "25000001");
    assert.equal(p.valueUpdatedTs.toString(), "1700000000");
    assert.equal(p.bump, 254);
    assert.equal(p.vaultAuthorityBump, 253);
  });

  it("decodes a WithdrawalTicket and maps the status enum", () => {
    const position = Keypair.generate().publicKey;
    const buf = Buffer.alloc(WITHDRAWAL_TICKET_SIZE);
    WITHDRAWAL_TICKET_DISCRIMINATOR.copy(buf, 0);
    position.toBuffer().copy(buf, 8);
    buf.writeBigUInt64LE(1000n, 40); // shares
    buf.writeBigUInt64LE(990n, 48); // min_amount_out
    buf.writeBigInt64LE(1_800_000_000n, 56); // unlock_ts
    buf.writeUInt8(1, 64); // status = Pending
    buf.writeBigInt64LE(1_700_000_000n, 65); // created_ts
    buf.writeUInt8(252, 73); // bump

    const t = decodeWithdrawalTicket(buf);
    assert.equal(t.position.toBase58(), position.toBase58());
    assert.equal(t.shares.toString(), "1000");
    assert.equal(t.minAmountOut.toString(), "990");
    assert.equal(t.unlockTs.toString(), "1800000000");
    assert.equal(t.status, "pending");
    assert.equal(t.createdTs.toString(), "1700000000");
    assert.equal(t.bump, 252);
  });

  it("decodes an AdapterEntry (trimmed name + status)", () => {
    const programId = Keypair.generate().publicKey;
    const baseMint = Keypair.generate().publicKey;
    const proposedBy = Keypair.generate().publicKey;
    const buf = Buffer.alloc(ADAPTER_ENTRY_SIZE);
    ADAPTER_ENTRY_DISCRIMINATOR.copy(buf, 0);
    programId.toBuffer().copy(buf, 8);
    baseMint.toBuffer().copy(buf, 40);
    buf.writeUInt8(1, 72); // status = Active
    Buffer.from("kamino").copy(buf, 73); // name padded with zeros
    buf.writeUInt16LE(1, 105); // version
    buf.writeUInt8(0, 107); // risk_tier
    buf.writeUInt8(9, 108); // remaining_accounts_hint
    proposedBy.toBuffer().copy(buf, 109);
    buf.writeBigInt64LE(1_690_000_000n, 141); // added_ts
    buf.writeUInt8(255, 149); // bump

    const e = decodeAdapterEntry(buf);
    assert.equal(e.programId.toBase58(), programId.toBase58());
    assert.equal(e.baseMint.toBase58(), baseMint.toBase58());
    assert.equal(e.status, "active");
    assert.equal(e.name, "kamino");
    assert.equal(e.version, 1);
    assert.equal(e.riskTier, 0);
    assert.equal(e.remainingAccountsHint, 9);
    assert.equal(e.proposedBy.toBase58(), proposedBy.toBase58());
    assert.equal(e.addedTs.toString(), "1690000000");
    assert.equal(e.bump, 255);
  });

  it("rejects a wrong discriminator", () => {
    const buf = Buffer.alloc(POSITION_SIZE); // all zeros => wrong disc
    assert.throws(() => decodePosition(buf), /discriminator mismatch/);
  });
});
