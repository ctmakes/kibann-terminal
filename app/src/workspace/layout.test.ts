import { describe, expect, it } from "vitest";
import {
  DIVIDER,
  cycle,
  dividers,
  equalize,
  leaf,
  neighbor,
  panes,
  ratioAt,
  rects,
  remove,
  resize,
  split,
  type Tree,
} from "./layout";

const BOX = { x: 0, y: 0, w: 801, h: 401 };

/** 1 | (2 over 3) */
function three(): Tree {
  return split(split(leaf(1), 1, "row", 2), 2, "column", 3);
}

describe("split and remove", () => {
  it("puts the new pane right of or below the target", () => {
    const t = three();
    expect(panes(t)).toEqual([1, 2, 3]);
    const r = rects(t, BOX);
    expect(r.get(1)).toEqual({ x: 0, y: 0, w: 400, h: 401 });
    expect(r.get(2)).toEqual({ x: 401, y: 0, w: 400, h: 200 });
    expect(r.get(3)).toEqual({ x: 401, y: 201, w: 400, h: 200 });
  });

  it("gives a closed pane's space to its sibling", () => {
    const t = remove(three(), 2)!;
    expect(panes(t)).toEqual([1, 3]);
    expect(rects(t, BOX).get(3)).toEqual({ x: 401, y: 0, w: 400, h: 401 });
    expect(remove(remove(t, 3)!, 1)).toBeNull();
  });

  it("ignores panes that aren't there", () => {
    const t = three();
    expect(split(t, 9, "row", 10)).toBe(t);
    expect(remove(t, 9)).toBe(t);
  });
});

describe("resizing", () => {
  it("moves the nearest divider in that direction", () => {
    let t = three();
    t = resize(t, BOX, 1, "right", 80);
    expect(rects(t, BOX).get(1)!.w).toBe(480);
    // Pane 2 is right of the divider: moving left grows it.
    t = resize(t, BOX, 2, "left", 80);
    expect(rects(t, BOX).get(1)!.w).toBe(400);
    // Pane 3 resizes the column split it is in, not the row split.
    t = resize(t, BOX, 3, "up", 100);
    expect(rects(t, BOX).get(2)!.h).toBe(100);
    // No vertical divider for pane 1.
    expect(resize(t, BOX, 1, "down", 10)).toBe(t);
  });

  it("keeps both sides visible", () => {
    const t = resize(three(), BOX, 1, "right", 5000);
    expect(rects(t, BOX).get(2)!.w).toBeGreaterThan(30);
  });

  it("equalizes runs of same-direction splits", () => {
    // 1 | 2 | 3 made by splitting right twice from the left pane's side.
    let t = split(split(leaf(1), 1, "row", 2), 2, "row", 3);
    t = equalize(t);
    const w = [1, 2, 3].map((p) => rects(t, { x: 0, y: 0, w: 902, h: 100 }).get(p)!.w);
    expect(w).toEqual([300, 300, 300]);
  });

  it("turns a pointer position into a ratio", () => {
    const t = three();
    const [d] = dividers(t, BOX);
    expect(d.rect).toEqual({ x: 400, y: 0, w: DIVIDER, h: 401 });
    expect(ratioAt(d, 200.5, 0)).toBeCloseTo(0.25);
  });
});

describe("focus movement", () => {
  it("finds the pane on each side", () => {
    const t = three();
    expect(neighbor(t, BOX, 1, "right")).toBe(2); // overlaps both; 2 comes first
    expect(neighbor(t, BOX, 3, "left")).toBe(1);
    expect(neighbor(t, BOX, 2, "down")).toBe(3);
    expect(neighbor(t, BOX, 3, "up")).toBe(2);
    expect(neighbor(t, BOX, 1, "left")).toBeNull();
    expect(neighbor(t, BOX, 2, "right")).toBeNull();
  });

  it("prefers the pane overlapping most", () => {
    // 1 | (2 over 3), with 3 much taller than 2.
    let t = three();
    t = resize(t, BOX, 2, "down", -150); // divider up: 2 is short
    expect(neighbor(t, BOX, 1, "right")).toBe(3);
  });

  it("cycles in reading order", () => {
    const t = three();
    expect(cycle(t, 1, 1)).toBe(2);
    expect(cycle(t, 3, 1)).toBe(1);
    expect(cycle(t, 1, -1)).toBe(3);
  });
});
