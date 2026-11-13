// Agent status in the workspace: a dot on each agent pane and on each tab,
// from the daemon's status map (`ServerMsg::Status`).

import type { AgentState, PaneStatus } from "../proto";
import type { PaneId } from "./layout";

/** Every pane with an agent, by pane id (JSON object keys are strings). */
export type StatusMap = Record<string, PaneStatus>;

export const DOT_COLORS: Partial<Record<AgentState, string>> = {
  working: "var(--k-working, #3fb950)",
  blocked: "var(--k-blocked, #e3a008)",
  done: "var(--k-done, #58a6ff)",
};

/** Most urgent first; idle shows nothing. */
const RANK: AgentState[] = ["blocked", "done", "working"];

/** The state a dot shows for `pane`, if any. */
export function dotFor(map: StatusMap, pane: PaneId): AgentState | null {
  const s = map[String(pane)]?.state;
  return s && s !== "idle" ? s : null;
}

/** The tab's dot: its most urgent pane (blocked > done > working). */
export function rollUp(map: StatusMap, panes: PaneId[]): AgentState | null {
  const states = new Set(panes.map((p) => dotFor(map, p)));
  return RANK.find((s) => states.has(s)) ?? null;
}

/** How many panes wait for the user (the Dock badge). */
export function blockedCount(map: StatusMap): number {
  return Object.values(map).filter((s) => s.state === "blocked").length;
}

const STYLE_ID = "kibann-status-style";

/** The pulse for working dots, once per document. */
export function installStyle(doc: Document = document): void {
  if (doc.getElementById(STYLE_ID)) return;
  const style = doc.createElement("style");
  style.id = STYLE_ID;
  style.textContent = `
@keyframes kibann-pulse { 0%, 100% { opacity: 1 } 50% { opacity: 0.35 } }
.agent-dot[data-state="working"] { animation: kibann-pulse 1.6s ease-in-out infinite }
.kibann-hidden .agent-dot { animation-play-state: paused }
@media (prefers-reduced-motion: reduce) { .agent-dot { animation: none !important } }
`;
  doc.head.append(style);
}

/** A dot element (hidden until it has a state). */
export function makeDot(extra: Partial<CSSStyleDeclaration> = {}): HTMLSpanElement {
  const d = document.createElement("span");
  d.className = "agent-dot";
  Object.assign(d.style, {
    display: "none",
    width: "8px",
    height: "8px",
    borderRadius: "50%",
    flex: "0 0 auto",
    pointerEvents: "none",
    ...extra,
  });
  return d;
}

export function showDot(dot: HTMLElement, state: AgentState | null, agent?: string): void {
  if (!state) {
    dot.style.display = "none";
    delete dot.dataset.state;
    dot.title = "";
    return;
  }
  dot.style.display = "block";
  dot.style.background = DOT_COLORS[state] ?? "transparent";
  dot.dataset.state = state;
  dot.title = agent ? `${agent}: ${state}` : state;
}
