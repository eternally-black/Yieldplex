// M8 verify — confirm the devnet deployment is live: every program account is executable, and the
// registry holds >= 5 Active adapter entries. Exits non-zero on any failure (CI/gate friendly).
//   ANCHOR_PROVIDER_URL=https://api.devnet.solana.com node scripts/verify-devnet.mjs
import { Connection, PublicKey } from "@solana/web3.js";

const URL = process.env.ANCHOR_PROVIDER_URL || "https://api.devnet.solana.com";

const REGISTRY = new PublicKey("3ehQoDePP3eULnSKxgHc6DvLAwEQNeVHvJYzWPXoQyUD");
const PROGRAMS = {
  ya_registry: "3ehQoDePP3eULnSKxgHc6DvLAwEQNeVHvJYzWPXoQyUD",
  ya_dispatcher: "2aY1hBVBJJmX8uSgB4aqhuS2xeDaGCc3d55KE2Mbvvgs",
  ya_adapter_kamino: "BwyrWhHa86dCyRghZn9EDK2ZxfhpBH4tr5NVoBJ3hTs5",
  ya_adapter_marginfi: "36CgQYZFxZQHzyMrn3NJRXR9jsVoYH44WitqGohoBGoi",
  ya_adapter_jupiter_jlp: "9fqh4833yoSJoPzpsucHY2SbUafVfHcC48RLQhTTahsB",
  ya_adapter_maple: "Ck9mwpX9kAjycbtN7jhD3s9xdHzUS2dwuV43g3BuBnD",
  ya_adapter_drift_if: "8MYJzh7Fm1q6QcrXNZNvCetoLkv1tfxjBDbrZXTFVjLs",
};
const ADAPTER_ENTRY_SIZE = 150;
const ADAPTER_STATUS = ["proposed", "active", "paused", "deprecated"];

async function main() {
  const connection = new Connection(URL, "confirmed");
  let failures = 0;
  const fail = (m) => { console.error(`  x ${m}`); failures++; };
  const ok = (m) => console.log(`  + ${m}`);

  console.log(`cluster: ${URL}`);
  console.log("=== program accounts executable ===");
  for (const [name, id] of Object.entries(PROGRAMS)) {
    const info = await connection.getAccountInfo(new PublicKey(id));
    if (!info) fail(`${name} (${id}) - account not found`);
    else if (!info.executable) fail(`${name} (${id}) - not executable`);
    else ok(`${name} (${id})`);
  }

  console.log("=== registry AdapterEntry accounts ===");
  const accounts = await connection.getProgramAccounts(REGISTRY, { filters: [{ dataSize: ADAPTER_ENTRY_SIZE }] });
  let active = 0;
  for (const { pubkey, account } of accounts) {
    const d = account.data;
    const programId = new PublicKey(d.subarray(8, 40)).toBase58();
    const status = ADAPTER_STATUS[d[72]] ?? `unknown(${d[72]})`;
    const nameRaw = d.subarray(73, 105);
    const end = nameRaw.indexOf(0);
    const name = nameRaw.subarray(0, end === -1 ? 32 : end).toString("utf8");
    if (status === "active") active++;
    console.log(`  ${status === "active" ? "+" : "."} ${name.padEnd(12)} ${status.padEnd(10)} ${programId}  (entry ${pubkey.toBase58()})`);
  }
  if (active < 5) fail(`expected >= 5 Active adapters, found ${active}`);
  else ok(`${active} Active adapters`);

  console.log(failures === 0 ? "\nDEVNET OK" : `\nDEVNET FAILED (${failures} problem(s))`);
  return failures;
}

main()
  .then((failures) => { process.exitCode = failures === 0 ? 0 : 1; })
  .catch((e) => { console.error(e); process.exitCode = 1; });
