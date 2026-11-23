// Checks that parts/kibann.d.ts (what part authors read) describes what the
// window sends (proto's generated part types). Compiled by
// `npm run typecheck` with tsconfig.parts.json, never bundled.

/// <reference path="../../../parts/kibann.d.ts" />
import type { AgentState as ProtoAgentState, PartState, PartTheme } from "../proto";
import type { PartPane } from "../proto/generated/PartPane";
import type { PartTab } from "../proto/generated/PartTab";
import type { PartAction } from "./parts";

type Same<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false;

const checks: [
  Same<AgentState, ProtoAgentState>,
  Same<KibannPane, PartPane>,
  Same<KibannTab, PartTab>,
  Same<KibannState, PartState>,
  Same<KibannTheme, PartTheme>,
  Same<KibannAction[0], PartAction>,
] = [true, true, true, true, true, true];
export default checks;
