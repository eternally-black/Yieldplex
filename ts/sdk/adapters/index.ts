export * from "./types";
export { kaminoAdapter, KAMINO_ADAPTER_PROGRAM_ID } from "./kamino";
export { marginfiAdapter, MARGINFI_ADAPTER_PROGRAM_ID } from "./marginfi";
export { jlpAdapter, JLP_ADAPTER_PROGRAM_ID } from "./jlp";
export { mapleAdapter, MAPLE_ADAPTER_PROGRAM_ID } from "./maple";
export {
  driftIfStandinAdapter, COOLDOWN_STANDIN_PROGRAM_ID, DRIFT_IF_COOLDOWN_SECONDS,
} from "./drift-if";

import { kaminoAdapter } from "./kamino";
import { marginfiAdapter } from "./marginfi";
import { jlpAdapter } from "./jlp";
import { mapleAdapter } from "./maple";
import { driftIfStandinAdapter } from "./drift-if";
import { AdapterDef } from "./types";

/** The instant, fork-runnable reference adapters (live-protocol value diff=0). */
export const instantAdapters: Record<string, AdapterDef> = {
  kamino: kaminoAdapter,
  marginfi: marginfiAdapter,
  jlp: jlpAdapter,
  maple: mapleAdapter,
};

/** Every reference adapter def, keyed by label (incl. the two-phase Drift stand-in). */
export const referenceAdapters: Record<string, AdapterDef> = {
  ...instantAdapters,
  "drift-standin": driftIfStandinAdapter,
};
