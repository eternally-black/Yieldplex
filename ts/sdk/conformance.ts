// Re-export the parametrized conformance suite so a third-party adapter author can self-certify
// against the Yield Adapter Standard in one import. The canonical implementation lives next to the
// test harness (it drives the dispatcher with the shared provider/payer context); the SDK surfaces
// it as part of the standard's public tooling.
export { runConformance } from "../../tests/conformance/runConformance";
export type { ConformanceConfig } from "../../tests/conformance/runConformance";
