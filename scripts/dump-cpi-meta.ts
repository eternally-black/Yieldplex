// Dump CPI metadata (discriminator + args + account order/mut/signer) for the exact
// instructions each adapter targets, plus field layouts for the accounts current_value reads.
// This is the source of truth for the MANUAL-CPI adapters (marginfi, drift) and for the
// SDK account-builders of all five. Writes a markdown report to idls/CPI-META.md.
import { readFileSync, writeFileSync } from "node:fs";

const root = new URL("../idls/", import.meta.url);
const load = (f) => JSON.parse(readFileSync(new URL(f, root), "utf8"));

const TARGETS = {
  kamino_lend: { ix: ["deposit_reserve_liquidity", "redeem_reserve_collateral", "refresh_reserve"], acct: ["Reserve"] },
  marginfi: { ix: ["marginfi_account_initialize", "lending_account_deposit", "lending_account_withdraw"], acct: ["MarginfiAccount", "Bank"] },
  jupiter_perps: { ix: ["add_liquidity2", "remove_liquidity2"], acct: ["Pool", "Custody"] },
  drift: { ix: ["initialize_user_stats", "initialize_insurance_fund_stake", "add_insurance_fund_stake", "request_remove_insurance_fund_stake", "remove_insurance_fund_stake", "cancel_request_remove_insurance_fund_stake"], acct: ["InsuranceFundStake", "SpotMarket", "UserStats"] },
  syrup_swap_pool: { ix: ["swap", "swap_v2"], acct: ["Whirlpool"] },
};

function ty(t) {
  if (t == null) return "?";
  if (typeof t === "string") return t;
  if (t.defined) return typeof t.defined === "string" ? t.defined : t.defined.name;
  if (t.option) return `Option<${ty(t.option)}>`;
  if (t.vec) return `Vec<${ty(t.vec)}>`;
  if (t.array) return `[${ty(t.array[0])}; ${t.array[1]}]`;
  return JSON.stringify(t);
}
function acctFlags(a) {
  const f = [];
  if (a.writable || a.isMut) f.push("mut");
  if (a.signer || a.isSigner) f.push("signer");
  if (a.optional || a.isOptional) f.push("optional");
  if (a.pda) f.push("pda");
  return f.length ? ` (${f.join(",")})` : "";
}

let md = "# CPI metadata (auto-dumped from vendored IDLs)\n\nManual-CPI adapters (marginfi, drift) build `Instruction{ data: discriminator ++ borsh(args), accounts }`.\n";

for (const [name, want] of Object.entries(TARGETS)) {
  let j;
  try { j = load(`${name}.json`); } catch (e) { md += `\n## ${name}\nLOAD ERROR: ${e.message}\n`; continue; }
  md += `\n## ${name}  (${j.address || j.metadata?.address || "?"})\n`;
  const ixs = j.instructions || [];
  for (const wi of want.ix) {
    const ix = ixs.find((x) => x.name === wi);
    if (!ix) { md += `\n### ix ${wi} — NOT FOUND\n`; continue; }
    md += `\n### ix \`${wi}\`\n`;
    md += `- discriminator: [${(ix.discriminator || []).join(",")}]\n`;
    md += `- args: ${(ix.args || []).map((a) => `${a.name}: ${ty(a.type)}`).join(", ") || "(none)"}\n`;
    md += `- accounts (${(ix.accounts || []).length}):\n`;
    (ix.accounts || []).forEach((a, i) => { md += `    ${i}. ${a.name}${acctFlags(a)}\n`; });
  }
  const accts = j.accounts || [];
  const types = j.types || [];
  for (const wa of want.acct) {
    const ac = accts.find((x) => x.name === wa);
    const tdef = types.find((x) => x.name === wa);
    if (!ac && !tdef) { md += `\n### account ${wa} — NOT FOUND\n`; continue; }
    md += `\n### account \`${wa}\``;
    if (ac?.discriminator) md += `  discriminator: [${ac.discriminator.join(",")}]`;
    md += `\n`;
    const fields = tdef?.type?.fields || [];
    if (fields.length) {
      md += `- fields:\n`;
      for (const fl of fields) md += `    - ${fl.name}: ${ty(fl.type)}\n`;
    } else {
      md += `- (type def not found inline; kind=${tdef?.type?.kind || "?"})\n`;
    }
  }
}

const outUrl = new URL("CPI-META.md", root);
writeFileSync(outUrl, md);
console.log(md);
console.log("\n--- written to idls/CPI-META.md ---");
