// Probe the @anchor-lang/core API surface to confirm it's a web3.js-compatible rename of @coral-xyz/anchor.
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const a = require("@anchor-lang/core");
console.log("version:", require("@anchor-lang/core/package.json").version);
for (const k of ["Program", "AnchorProvider", "BN", "web3", "Wallet", "BorshCoder", "BorshAccountsCoder", "EventParser"]) {
  console.log(`  ${k}:`, typeof a[k]);
}
console.log("  web3.Connection:", typeof (a.web3 && a.web3.Connection));
console.log("  web3.PublicKey:", typeof (a.web3 && a.web3.PublicKey));
