// The split tree of one tab: pure data and functions, no DOM.
//
// A split divides its box between `a` (left or top) and `b` (right or
// bottom). "row" puts them side by side (split right); "column" stacks them
// (split down). Trees are immutable: every change returns a new tree.

export type PaneId = number;
export type Dir = "row" | "column";
export type Direction = "left" | "right" | "up" | "down";

export type Tree =
  | { kind: "leaf"; pane: PaneId }
  | { kind: "split"; dir: Dir; ratio: number; a: Tree; b: Tree };
// Same shape as the daemon's (`proto/generated/Tree`).

/** Steps from the root to a node. */
export type Path = Array<"a" | "b">;

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Width of the line between two panes, in CSS px. */
export const DIVIDER = 1;
/** Smallest share of a split either side may shrink to. */
const MIN_RATIO = 0.05;

export const leaf = (pane: PaneId): Tree => ({ kind: "leaf", pane });

/** Panes in reading order: left before right, top before bottom. */
export function panes(t: Tree): PaneId[] {
  return t.kind === "leaf" ? [t.pane] : [...panes(t.a), ...panes(t.b)];
}

export function pathOf(t: Tree, pane: PaneId): Path | null {
  if (t.kind === "leaf") return t.pane === pane ? [] : null;
  const a = pathOf(t.a, pane);
  if (a) return ["a", ...a];
  const b = pathOf(t.b, pane);
  return b ? ["b", ...b] : null;
}

function at(t: Tree, path: Path): Tree {
  return path.reduce((n, step) => (n.kind === "split" ? n[step] : n), t);
}

function replace(t: Tree, path: Path, f: (n: Tree) => Tree): Tree {
  if (path.length === 0) return f(t);
  if (t.kind === "leaf") return t;
  const [step, ...rest] = path;
  return { ...t, [step]: replace(t[step], rest, f) };
}

/** Split `target` in two, putting `pane` right of it ("row") or below it ("column"). */
export function split(t: Tree, target: PaneId, dir: Dir, pane: PaneId): Tree {
  const path = pathOf(t, target);
  if (!path) return t;
  return replace(t, path, (n) => ({ kind: "split", dir, ratio: 0.5, a: n, b: leaf(pane) }));
}

/** Remove `pane`; its sibling takes the space. Null when it was the last pane. */
export function remove(t: Tree, pane: PaneId): Tree | null {
  const path = pathOf(t, pane);
  if (!path) return t;
  if (path.length === 0) return null;
  const parent = path.slice(0, -1);
  const sibling = path[path.length - 1] === "a" ? "b" : "a";
  return replace(t, parent, (n) => (n.kind === "split" ? n[sibling] : n));
}

const clampRatio = (r: number) => Math.min(1 - MIN_RATIO, Math.max(MIN_RATIO, r));

/** Set the ratio of the split at `path`. */
export function setRatio(t: Tree, path: Path, ratio: number): Tree {
  return replace(t, path, (n) => (n.kind === "split" ? { ...n, ratio: clampRatio(ratio) } : n));
}

/**
 * Move the divider nearest `pane` on its `toward` side by `delta` px, within
 * `box`. Moving it right or down makes the left or top side bigger. Returns
 * the tree unchanged when there is no divider that way.
 */
export function resize(t: Tree, box: Rect, pane: PaneId, toward: Direction, delta: number): Tree {
  const r = resizeTarget(t, box, pane, toward, delta);
  return r ? setRatio(t, r.path, r.ratio) : t;
}

/** The split [`resize`] would change and its new ratio (unclamped), or
 * null when there is no divider that way. */
export function resizeTarget(
  t: Tree,
  box: Rect,
  pane: PaneId,
  toward: Direction,
  delta: number,
): { path: Path; ratio: number } | null {
  const path = pathOf(t, pane);
  if (!path) return null;
  const dir: Dir = toward === "left" || toward === "right" ? "row" : "column";
  const sign = toward === "left" || toward === "up" ? -1 : 1;
  // Nearest ancestor split in that orientation.
  for (let i = path.length - 1; i >= 0; i--) {
    const p = path.slice(0, i);
    const n = at(t, p);
    if (n.kind !== "split" || n.dir !== dir) continue;
    const r = boxOf(t, box, p);
    const size = (dir === "row" ? r.w : r.h) - DIVIDER;
    if (size <= 0) return null;
    return { path: p, ratio: clampRatio(n.ratio + (sign * delta) / size) };
  }
  return null;
}

/** Give every pane in a run of same-direction splits an equal share. */
export function equalize(t: Tree): Tree {
  if (t.kind === "leaf") return t;
  const weight = (n: Tree, dir: Dir): number =>
    n.kind === "split" && n.dir === dir ? weight(n.a, dir) + weight(n.b, dir) : 1;
  const wa = weight(t.a, t.dir);
  const wb = weight(t.b, t.dir);
  return { ...t, ratio: wa / (wa + wb), a: equalize(t.a), b: equalize(t.b) };
}

/** The two halves of `box` for a split, with the divider between them. */
export function halves(box: Rect, dir: Dir, ratio: number): [Rect, Rect, Rect] {
  if (dir === "row") {
    const aw = Math.round((box.w - DIVIDER) * ratio);
    const a = { x: box.x, y: box.y, w: aw, h: box.h };
    const d = { x: box.x + aw, y: box.y, w: DIVIDER, h: box.h };
    const b = { x: box.x + aw + DIVIDER, y: box.y, w: box.w - aw - DIVIDER, h: box.h };
    return [a, d, b];
  }
  const ah = Math.round((box.h - DIVIDER) * ratio);
  const a = { x: box.x, y: box.y, w: box.w, h: ah };
  const d = { x: box.x, y: box.y + ah, w: box.w, h: DIVIDER };
  const b = { x: box.x, y: box.y + ah + DIVIDER, w: box.w, h: box.h - ah - DIVIDER };
  return [a, d, b];
}

/** The box of the node at `path`. */
export function boxOf(t: Tree, box: Rect, path: Path): Rect {
  let n = t;
  let r = box;
  for (const step of path) {
    if (n.kind !== "split") break;
    const [a, , b] = halves(r, n.dir, n.ratio);
    r = step === "a" ? a : b;
    n = n[step];
  }
  return r;
}

/** Where each pane goes inside `box`. */
export function rects(t: Tree, box: Rect, out = new Map<PaneId, Rect>()): Map<PaneId, Rect> {
  if (t.kind === "leaf") {
    out.set(t.pane, box);
    return out;
  }
  const [a, , b] = halves(box, t.dir, t.ratio);
  rects(t.a, a, out);
  rects(t.b, b, out);
  return out;
}

export interface DividerInfo {
  path: Path;
  dir: Dir;
  /** The line itself. */
  rect: Rect;
  /** The split's whole box, to turn a pointer position into a ratio. */
  box: Rect;
}

export function dividers(t: Tree, box: Rect, path: Path = [], out: DividerInfo[] = []): DividerInfo[] {
  if (t.kind === "leaf") return out;
  const [a, d, b] = halves(box, t.dir, t.ratio);
  out.push({ path, dir: t.dir, rect: d, box });
  dividers(t.a, a, [...path, "a"], out);
  dividers(t.b, b, [...path, "b"], out);
  return out;
}

/** The ratio that puts a split's divider at pointer position (x, y). */
export function ratioAt(d: DividerInfo, x: number, y: number): number {
  return d.dir === "row"
    ? (x - d.box.x - DIVIDER / 2) / (d.box.w - DIVIDER)
    : (y - d.box.y - DIVIDER / 2) / (d.box.h - DIVIDER);
}

/**
 * The pane next to `from` in `toward`: among panes entirely on that side
 * that overlap it across, the nearest, then the one overlapping most.
 */
export function neighbor(t: Tree, box: Rect, from: PaneId, toward: Direction): PaneId | null {
  const all = rects(t, box);
  const c = all.get(from);
  if (!c) return null;
  const eps = 0.5;
  let best: { pane: PaneId; dist: number; overlap: number } | null = null;
  for (const [pane, r] of all) {
    if (pane === from) continue;
    let dist: number;
    let overlap: number;
    const across = (a0: number, a1: number, b0: number, b1: number) => Math.min(a1, b1) - Math.max(a0, b0);
    switch (toward) {
      case "right":
        dist = r.x - (c.x + c.w);
        overlap = across(c.y, c.y + c.h, r.y, r.y + r.h);
        break;
      case "left":
        dist = c.x - (r.x + r.w);
        overlap = across(c.y, c.y + c.h, r.y, r.y + r.h);
        break;
      case "down":
        dist = r.y - (c.y + c.h);
        overlap = across(c.x, c.x + c.w, r.x, r.x + r.w);
        break;
      case "up":
        dist = c.y - (r.y + r.h);
        overlap = across(c.x, c.x + c.w, r.x, r.x + r.w);
        break;
    }
    if (dist < -eps || overlap <= 0) continue;
    if (!best || dist < best.dist - eps || (Math.abs(dist - best.dist) <= eps && overlap > best.overlap)) {
      best = { pane, dist, overlap };
    }
  }
  return best?.pane ?? null;
}

/** The pane after (`step` 1) or before (-1) `from` in reading order, wrapping. */
export function cycle(t: Tree, from: PaneId, step: 1 | -1): PaneId {
  const all = panes(t);
  const i = all.indexOf(from);
  return all[(i + step + all.length) % all.length];
}
