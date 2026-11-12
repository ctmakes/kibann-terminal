import { describe, expect, it } from "vitest";
import { leaf, split, type Tree } from "./layout";
import { mapLeaves, parseSaved, prune, type Saved } from "./snapshot";

/** 1 | (2 over 3) */
const three = (): Tree => split(split(leaf(1), 1, "row", 2), 2, "column", 3);

const saved = (...tabs: Saved["tabs"]): Saved => ({ version: 1, tabs, active: tabs.length - 1 });

describe("mapLeaves", () => {
  it("renames leaves and lets siblings take dropped ones' space", () => {
    expect(mapLeaves(three(), (id) => id * 10)).toEqual(
      split(split(leaf(10), 10, "row", 20), 20, "column", 30),
    );
    expect(mapLeaves(three(), (id) => (id === 2 ? null : id))).toEqual(split(leaf(1), 1, "row", 3));
    expect(mapLeaves(three(), () => null)).toBeNull();
  });
});

describe("prune", () => {
  it("keeps a layout whose sessions all run", () => {
    const s = saved({ tree: three(), focused: 3, zoomed: null });
    expect(prune(s, [1, 2, 3])).toEqual({ saved: s, extra: [] });
  });

  it("drops ended sessions and moves focus and zoom off them", () => {
    const s = saved({ tree: three(), focused: 2, zoomed: 2 });
    const { saved: out } = prune(s, [1, 3]);
    expect(out.tabs[0].tree).toEqual(split(leaf(1), 1, "row", 3));
    expect(out.tabs[0].focused).toBe(1);
    expect(out.tabs[0].zoomed).toBeNull();
  });

  it("drops empty tabs and keeps the active tab or the one before it", () => {
    const s: Saved = {
      version: 1,
      tabs: [
        { tree: leaf(1), focused: 1, zoomed: null },
        { tree: leaf(2), focused: 2, zoomed: null },
        { tree: leaf(3), focused: 3, zoomed: null },
      ],
      active: 1,
    };
    expect(prune(s, [1, 2, 3]).saved.active).toBe(1);
    const gone = prune(s, [1, 3]).saved;
    expect(gone.tabs.map((t) => t.focused)).toEqual([1, 3]);
    expect(gone.active).toBe(0);
    expect(prune(s, []).saved).toEqual({ version: 1, tabs: [], active: 0 });
  });

  it("returns running sessions the layout doesn't know", () => {
    const s = saved({ tree: leaf(1), focused: 1, zoomed: null });
    expect(prune(s, [1, 4, 5]).extra).toEqual([4, 5]);
  });

  it("uses a session only once", () => {
    const s = saved({ tree: leaf(1), focused: 1, zoomed: null }, { tree: leaf(1), focused: 1, zoomed: null });
    expect(prune(s, [1]).saved.tabs).toHaveLength(1);
  });
});

describe("parseSaved", () => {
  it("reads what was saved and rejects anything else", () => {
    const s = saved({ tree: three(), focused: 1, zoomed: null });
    expect(parseSaved(JSON.stringify(s))).toEqual(s);
    expect(parseSaved(null)).toBeNull();
    expect(parseSaved("not json")).toBeNull();
    expect(parseSaved(JSON.stringify({ ...s, version: 2 }))).toBeNull();
    expect(parseSaved(JSON.stringify({ version: 1, tabs: [{ tree: { kind: "leaf" }, focused: 1 }], active: 0 }))).toBeNull();
  });
});
