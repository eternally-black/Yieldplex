// Inspect vendored IDLs: report spec/format, address, instruction & account counts,
// whether instructions carry 8-byte discriminators (new Anchor 0.30+ spec needed by declare_program!),
// and the specific instructions each adapter will CPI into.
import { readFileSync, readdirSync } from "node:fs";

const WANT = {
  kamino_lend: ["init_reserve", "deposit_reserve_liquidity", "redeem_reserve_collateral", "refresh_reserve"],
  marginfi: ["marginfi_account_initialize", "lending_account_deposit", "lending_account_withdraw"],
  jupiter_perps: ["add_liquidity", "remove_liquidity", "add_liquidity2", "remove_liquidity2"],
  drift: ["initialize_user_stats", "initialize_insurance_fund_stake", "add_insurance_fund_stake",
          "request_remove_insurance_fund_stake", "remove_insurance_fund_stake", "cancel_request_remove_insurance_fund_stake"],
};

const dir = new URL("../idls/", import.meta.url);
let files;
try { files = readdirSync(dir).filter((f) => f.endsWith(".json")); }
catch { console.log("no idls/ dir"); process.exit(0); }

for (const f of files.sort()) {
  const name = f.replace(/\.json$/, "");
  let j;
  try { j = JSON.parse(readFileSync(new URL(f, dir), "utf8")); }
  catch (e) { console.log(`### ${name}: PARSE ERROR ${e.message}\n`); continue; }
  const ixs = j.instructions || [];
  const spec = j.metadata?.spec || (ixs[0]?.discriminator ? "0.30+ (has discriminators)" : "LEGACY (<0.30, no discriminators)");
  console.log(`### ${name}`);
  console.log(`  spec: ${spec}`);
  console.log(`  address: ${j.address || j.metadata?.address || "(none)"}`);
  console.log(`  name(meta): ${j.metadata?.name || j.name || "(none)"}   anchor ver: ${j.metadata?.version || j.version || "?"}`);
  console.log(`  instructions: ${ixs.length}   accounts: ${(j.accounts || []).length}   types: ${(j.types || []).length}   errors: ${(j.errors || []).length}`);
  console.log(`  ix0 has discriminator: ${!!ixs[0]?.discriminator}   account0 has discriminator: ${!!(j.accounts || [])[0]?.discriminator}`);
  const present = new Set(ixs.map((i) => i.name));
  const want = WANT[name] || [];
  if (want.length) {
    for (const w of want) console.log(`    ${present.has(w) ? "FOUND " : "missing"} ix: ${w}`);
  }
  console.log("");
}
