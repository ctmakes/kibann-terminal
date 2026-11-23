// Mirrors the tests in crates/daemon/src/layout.rs.
import { describe, expect, it } from "vitest";
import { panes } from "./layout";
import { Model } from "./model";

/** 1 | (2 over 3), in tab 1. */
function three(): Model {
  const m = new Model();
  m.newTab(1, true);
  m.split(1, "row", 2, true);
  m.split(2, "column", 3, true);
  return m;
}

describe("layout model", () => {
  it("splits right or below, focusing the new pane when asked", () => {
    const m = three();
    expect(panes(m.layout.tabs[0].tree)).toEqual([1, 2, 3]);
    expect(m.layout.tabs[0].focused).toBe(3);
    m.focus(1);
    expect(m.split(1, "column", 4, false)).toBe(1);
    expect(m.layout.tabs[0].focused).toBe(1);
    expect(m.split(9, "row", 5, false)).toBeNull();
  });

  it("removes panes, moving focus back, and drops emptied tabs", () => {
    const m = three();
    m.remove(3);
    expect(panes(m.layout.tabs[0].tree)).toEqual([1, 2]);
    expect(m.layout.tabs[0].focused).toBe(2);
    const t2 = m.newTab(4, true);
    const t3 = m.newTab(5, false);
    expect(m.layout.active).toBe(t2);
    m.remove(4);
    expect(m.layout.active).toBe(t3);
    m.remove(5);
    expect(m.layout.active).toBe(1);
    m.remove(1);
    m.remove(2);
    expect(m.layout).toEqual({ tabs: [], active: null });
    expect(m.newTab(6, false)).toBe(4);
  });

  it("focus shows the tab and ends zoom on another pane", () => {
    const m = three();
    m.newTab(4, true);
    m.zoom(1, 2);
    expect(m.layout.tabs[0].zoomed).toBe(2);
    m.focus(2);
    expect(m.layout.tabs[0].zoomed).toBe(2);
    expect(m.layout.active).toBe(1);
    m.focus(3);
    expect(m.layout.tabs[0].zoomed).toBeNull();
  });

  it("never zooms a lone pane", () => {
    const m = new Model();
    m.newTab(1, true);
    m.zoom(1, 1);
    expect(m.layout.tabs[0].zoomed).toBeNull();
    m.split(1, "row", 2, false);
    m.zoom(1, 2);
    m.remove(1);
    expect(m.layout.tabs[0].zoomed).toBeNull();
  });

  it("starts after the ids of a given layout", () => {
    const m = new Model({ tabs: [{ id: 7, tree: { kind: "leaf", pane: 1 }, focused: 1, zoomed: null }], active: 7 });
    expect(m.newTab(2, false)).toBe(8);
  });
});
