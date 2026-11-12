// The layout model the daemon owns (`crates/daemon/src/layout.rs`), in
// TypeScript for the browser harness, which has no daemon. Keep the two in
// step; `model.test.ts` mirrors the Rust tests.

import type { Layout, LayoutTab } from "../proto";
import { equalize, leaf, panes, pathOf, remove, setRatio, split, type Dir, type Path, type PaneId } from "./layout";

export type TabId = number;

export class Model {
  layout: Layout;
  private nextTab: number;

  constructor(layout: Layout = { tabs: [], active: null }) {
    this.layout = structuredClone(layout);
    this.nextTab = Math.max(0, ...layout.tabs.map((t) => t.id));
  }

  private tab(id: TabId): LayoutTab | undefined {
    return this.layout.tabs.find((t) => t.id === id);
  }

  tabOf(pane: PaneId): LayoutTab | undefined {
    return this.layout.tabs.find((t) => pathOf(t.tree, pane) !== null);
  }

  newTab(pane: PaneId, focus: boolean): TabId {
    const id = ++this.nextTab;
    this.layout.tabs.push({ id, tree: leaf(pane), focused: pane, zoomed: null });
    if (focus || this.layout.active === null) this.layout.active = id;
    return id;
  }

  split(target: PaneId, dir: Dir, pane: PaneId, focus: boolean): TabId | null {
    const tab = this.tabOf(target);
    if (!tab) return null;
    tab.tree = split(tab.tree, target, dir, pane);
    tab.zoomed = null;
    if (focus) {
      tab.focused = pane;
      this.layout.active = tab.id;
    }
    return tab.id;
  }

  remove(pane: PaneId): boolean {
    const i = this.layout.tabs.findIndex((t) => pathOf(t.tree, pane) !== null);
    if (i < 0) return false;
    const tab = this.layout.tabs[i];
    const at = panes(tab.tree).indexOf(pane);
    const tree = remove(tab.tree, pane);
    if (tree) {
      const left = panes(tree);
      tab.tree = tree;
      if (tab.zoomed === pane || left.length < 2) tab.zoomed = null;
      if (tab.focused === pane) tab.focused = left[Math.min(Math.max(0, at - 1), left.length - 1)];
    } else {
      this.layout.tabs.splice(i, 1);
      if (this.layout.active === tab.id) {
        const next = this.layout.tabs[Math.min(i, this.layout.tabs.length - 1)];
        this.layout.active = next?.id ?? null;
      }
    }
    return true;
  }

  focus(pane: PaneId): boolean {
    const tab = this.tabOf(pane);
    if (!tab) return false;
    if (tab.zoomed !== null && tab.zoomed !== pane) tab.zoomed = null;
    tab.focused = pane;
    this.layout.active = tab.id;
    return true;
  }

  activate(id: TabId): boolean {
    if (!this.tab(id)) return false;
    this.layout.active = id;
    return true;
  }

  setRatio(id: TabId, path: Path, ratio: number): boolean {
    const tab = this.tab(id);
    if (!tab || !Number.isFinite(ratio)) return false;
    tab.tree = setRatio(tab.tree, path, ratio);
    tab.zoomed = null;
    return true;
  }

  equalize(id: TabId): boolean {
    const tab = this.tab(id);
    if (!tab) return false;
    tab.tree = equalize(tab.tree);
    return true;
  }

  zoom(id: TabId, pane: PaneId | null): boolean {
    const tab = this.tab(id);
    if (!tab) return false;
    if (pane === null) {
      tab.zoomed = null;
      return true;
    }
    if (pathOf(tab.tree, pane) === null) return false;
    if (panes(tab.tree).length > 1) tab.zoomed = pane;
    tab.focused = pane;
    return true;
  }
}
