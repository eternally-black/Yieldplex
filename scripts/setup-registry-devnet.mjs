// M8 — initialize the registry on devnet and propose+approve the 5 reference adapters, then write
// deploy/devnet.json. Governance = guardian = the deployer wallet (swap in a Squads multisig with
// zero code changes). Run via deploy-devnet.sh (after the programs are deployed).
import * as anchor from "@anchor-lang/core";
import { Connection, Keypair, PublicKey, SystemProgram } from "@solana/web3.js";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.dirname(__dirname);
const URL = process.env.ANCHOR_PROVIDER_URL || "https://api.devnet.solana.com";
const WALLET = process.env.ANCHOR_WALLET || path.join(process.env.HOME, ".config/solana/id.json");
const USDC = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

const DISPATCHER = "2aY1hBVBJJmX8uSgB4aqhuS2xeDaGCc3d55KE2Mbvvgs";
const ADAPTERS = [
  { name: "kamino", programId: "BwyrWhHa86dCyRghZn9EDK2ZxfhpBH4tr5NVoBJ3hTs5", hint: 8 },
  { name: "marginfi", programId: "36CgQYZFxZQHzyMrn3NJRXR9jsVoYH44WitqGohoBGoi", hint: 5 },
  { name: "jupiter-jlp", programId: "9fqh4833yoSJoPzpsucHY2SbUafVfHcC48RLQhTTahsB", hint: 21 },
  { name: "maple", programId: "Ck9mwpX9kAjycbtN7jhD3s9xdHzUS2dwuV43g3BuBnD", hint: 10 },
  { name: "drift-if", programId: "8MYJzh7Fm1q6QcrXNZNvCetoLkv1tfxjBDbrZXTFVjLs", hint: 12 },
];

const payer = Keypair.fromSecretKey(Uint8Array.from(JSON.parse(fs.readFileSync(WALLET, "utf8"))));
const connection = new Connection(URL, "confirmed");
const provider = new anchor.AnchorProvider(connection, new anchor.Wallet(payer), { commitment: "confirmed" });
anchor.setProvider(provider);

const idl = (n) => JSON.parse(fs.readFileSync(path.join(REPO, "target", "idl", `${n}.json`), "utf8"));
const registry = new anchor.Program(idl("ya_registry"), provider);
const REGISTRY_PID = registry.programId;

const registryPda = PublicKey.findProgramAddressSync([Buffer.from("registry")], REGISTRY_PID)[0];
const entryPda = (programId) =>
  PublicKey.findProgramAddressSync([Buffer.from("adapter"), new PublicKey(programId).toBuffer()], REGISTRY_PID)[0];

async function main() {
  console.log(`registry program: ${REGISTRY_PID.toBase58()}`);
  console.log(`governance/guardian (wallet): ${payer.publicKey.toBase58()}`);

  if (!(await connection.getAccountInfo(registryPda))) {
    await registry.methods
      .initializeRegistry(payer.publicKey, payer.publicKey)
      .accounts({ registry: registryPda, payer: payer.publicKey, systemProgram: SystemProgram.programId })
      .rpc();
    console.log(`  initialized registry ${registryPda.toBase58()}`);
  } else {
    console.log(`  registry already initialized (${registryPda.toBase58()})`);
  }

  const adapterRecords = [];
  for (const a of ADAPTERS) {
    const entry = entryPda(a.programId);
    if (!(await connection.getAccountInfo(entry))) {
      await registry.methods
        .proposeAdapter(new PublicKey(a.programId), USDC, a.name, 1, 0, a.hint)
        .accounts({ registry: registryPda, adapterEntry: entry, governance: payer.publicKey, systemProgram: SystemProgram.programId })
        .rpc();
      await registry.methods
        .approveAdapter()
        .accounts({ registry: registryPda, adapterEntry: entry, governance: payer.publicKey })
        .rpc();
      console.log(`  proposed + approved ${a.name} (${a.programId})`);
    } else {
      console.log(`  ${a.name} entry already exists (${entry.toBase58()})`);
    }
    adapterRecords.push({ name: a.name, programId: a.programId, entry: entry.toBase58() });
  }

  const out = {
    cluster: "devnet",
    rpc: "https://api.devnet.solana.com",
    deployedAt: new Date().toISOString(),
    wallet: payer.publicKey.toBase58(),
    governance: payer.publicKey.toBase58(),
    guardian: payer.publicKey.toBase58(),
    baseMint: USDC.toBase58(),
    programs: {
      ya_registry: REGISTRY_PID.toBase58(),
      ya_dispatcher: DISPATCHER,
      ...Object.fromEntries(ADAPTERS.map((a) => [a.name, a.programId])),
    },
    registry: registryPda.toBase58(),
    adapters: adapterRecords,
    note: "Protocols are absent on devnet; deposit/withdraw execution is validated on mainnet-fork (tests/fork). This deployment proves the registry/dispatcher/adapter program ids are live and governance-gated.",
  };
  fs.mkdirSync(path.join(REPO, "deploy"), { recursive: true });
  fs.writeFileSync(path.join(REPO, "deploy", "devnet.json"), JSON.stringify(out, null, 2) + "\n");
  console.log(`  wrote deploy/devnet.json (${adapterRecords.length} adapters)`);
}

main().then(() => process.exit(0)).catch((e) => { console.error(e); process.exit(1); });
