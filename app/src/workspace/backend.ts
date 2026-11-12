// Where layout actions go. In the app it is the daemon (through Tauri); in
// the browser harness, `FakeBackend`, which keeps the same model in
// TypeScript. Either way the workspace changes only when a layout comes back
// (`Workspace.apply`), except for focus, which it moves at once.

import type { Layout, LayoutAction } from "../proto";
import type { PaneId } from "./layout";
import { Model, type TabId } from "./model";

/** The pane and tab an action made or acted on. */
export interface Acted {
  pane: PaneId | null;
  tab: TabId | null;
}

export interface LayoutBackend {
  act(action: LayoutAction): Promise<Acted>;
}

/** Focus changes the app makes itself aren't sent back to it (as the daemon
 * does): a late echo could undo a newer click. */
export function echoed(a: LayoutAction): boolean {
  return a.t !== "focus" && a.t !== "activate";
}

/**
 * The daemon's layout handling without a daemon: new panes get the next id,
 * and every change is delivered to `onLayout` a moment later.
 */
export class FakeBackend implements LayoutBackend {
  readonly model: Model;
  private nextPane: number;
  /** Which pane each new pane was opened from (for its directory). */
  readonly origin = new Map<PaneId, PaneId | null>();
  onLayout: (layout: Layout) => void = () => {};

  constructor(layout?: Layout) {
    this.model = new Model(layout);
    const all = this.model.layout.tabs.flatMap((t) => collect(t.tree));
    this.nextPane = Math.max(0, ...all);
  }

  get layout(): Layout {
    return structuredClone(this.model.layout);
  }

  /** An action from the app. */
  act(a: LayoutAction): Promise<Acted> {
    return this.apply(a, echoed(a));
  }

  /** An action from someone else (an agent with the CLI): always delivered. */
  external(a: LayoutAction): Promise<Acted> {
    return this.apply(a, true);
  }

  /** A pane's program exited. */
  exited(pane: PaneId): void {
    if (this.model.remove(pane)) this.deliver();
  }

  private apply(a: LayoutAction, deliver: boolean): Promise<Acted> {
    const m = this.model;
    let acted: Acted | null = null;
    switch (a.t) {
      case "new_tab": {
        const pane = ++this.nextPane;
        this.origin.set(pane, a.near ?? null);
        acted = { pane, tab: m.newTab(pane, a.focus) };
        break;
      }
      case "split": {
        if (!m.tabOf(a.pane)) break;
        const pane = ++this.nextPane;
        this.origin.set(pane, a.pane);
        acted = { pane, tab: m.split(a.pane, a.dir, pane, a.focus) };
        break;
      }
      case "close":
        if (m.remove(a.pane)) acted = { pane: a.pane, tab: null };
        break;
      case "focus":
        if (m.focus(a.pane)) acted = { pane: a.pane, tab: m.layout.active };
        break;
      case "activate":
        if (m.activate(a.tab)) acted = { pane: null, tab: a.tab };
        break;
      case "set_ratio":
        if (m.setRatio(a.tab, a.path, a.ratio)) acted = { pane: null, tab: a.tab };
        break;
      case "equalize":
        if (m.equalize(a.tab)) acted = { pane: null, tab: a.tab };
        break;
      case "zoom":
        if (m.zoom(a.tab, a.pane ?? null)) acted = { pane: a.pane ?? null, tab: a.tab };
        break;
    }
    if (!acted) return Promise.reject(new Error(`can't ${a.t}: no such pane or tab`));
    if (deliver) this.deliver();
    return Promise.resolve(acted);
  }

  private deliver(): void {
    const layout = this.layout;
    queueMicrotask(() => this.onLayout(layout));
  }
}

function collect(t: Layout["tabs"][number]["tree"]): number[] {
  return t.kind === "leaf" ? [t.pane] : [...collect(t.a), ...collect(t.b)];
}
