// The workspace as data the daemon keeps while the app is closed: each tab's
// split tree with session ids in place of pane ids. Pure; no DOM.

import { panes, type Tree } from "./layout";

export type SessionId = number;

export interface SavedTab {
  /** Leaves hold session ids. */
  tree: Tree;
  focused: SessionId;
  zoomed: SessionId | null;
}

export interface Saved {
  version: 1;
  tabs: SavedTab[];
  active: number;
}

export const EMPTY: Saved = { version: 1, tabs: [], active: 0 };

/** Replace each leaf's id with `f(id)`, dropping leaves where it is null
 * (their siblings take the space). Null when nothing is left. */
export function mapLeaves(t: Tree, f: (id: number) => number | null): Tree | null {
  if (t.kind === "leaf") {
    const id = f(t.pane);
    return id === null ? null : { kind: "leaf", pane: id };
  }
  const a = mapLeaves(t.a, f);
  const b = mapLeaves(t.b, f);
  if (a && b) return { ...t, a, b };
  return a ?? b;
}

function isTree(v: unknown): v is Tree {
  const t = v as Tree;
  if (!t || typeof t !== "object") return false;
  if (t.kind === "leaf") return Number.isInteger(t.pane);
  return (
    t.kind === "split" &&
    (t.dir === "row" || t.dir === "column") &&
    typeof t.ratio === "number" &&
    isTree(t.a) &&
    isTree(t.b)
  );
}

/** Read a saved layout; anything unexpected (an older format, a hand edit)
 * counts as none. */
export function parseSaved(json: string | null | undefined): Saved | null {
  if (!json) return null;
  try {
    const v = JSON.parse(json) as Saved;
    if (v?.version !== 1 || !Array.isArray(v.tabs) || !Number.isInteger(v.active)) return null;
    if (!v.tabs.every((t) => isTree(t.tree) && Number.isInteger(t.focused))) return null;
    return v;
  } catch {
    return null;
  }
}

/**
 * Fit a saved layout to the sessions still running: panes whose shell ended
 * go, as if closed; tabs left empty go; focus and zoom move to a pane that
 * is still there. Returns the live sessions the layout doesn't mention (to
 * open as tabs of their own).
 */
export function prune(saved: Saved, live: SessionId[]): { saved: Saved; extra: SessionId[] } {
  const alive = new Set(live);
  const seen = new Set<SessionId>();
  const tabs: SavedTab[] = [];
  let active = 0;
  saved.tabs.forEach((tab, i) => {
    const tree = mapLeaves(tab.tree, (id) => (alive.has(id) && !seen.has(id) ? id : null));
    if (!tree) return;
    const ids = panes(tree);
    for (const id of ids) seen.add(id);
    if (i <= saved.active) active = tabs.length;
    tabs.push({
      tree,
      focused: ids.includes(tab.focused) ? tab.focused : ids[0],
      zoomed: tab.zoomed !== null && ids.includes(tab.zoomed) && ids.length > 1 ? tab.zoomed : null,
    });
  });
  return {
    saved: { version: 1, tabs, active: Math.min(active, Math.max(0, tabs.length - 1)) },
    extra: live.filter((id) => !seen.has(id)),
  };
}
