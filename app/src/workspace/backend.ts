// Where layout actions go. In the app it is the daemon (through Tauri); in
// the browser harness, `FakeBackend`, which keeps the same model in
// TypeScript. Either way the workspace changes only when a layout comes back
// (`Workspace.apply`), except for focus, which it moves at once.

import type { AgentState, Layout, LayoutAction, PaneDir } from "../proto";
import type { PaneId } from "./layout";
import { Model, type TabId } from "./model";
import type { GitMap } from "./pane";
import type { StatusMap } from "./status";

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
  onStatus: (map: StatusMap) => void = () => {};
  onGit: (map: GitMap) => void = () => {};
  private places: GitMap = {};
  /** Agent status, as the daemon would keep it. */
  private status: StatusMap = {};

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

  /** A pane's agent is in `state` (null: no agent). Like the daemon,
   * working → idle on a pane that isn't the focused pane of the shown tab
   * becomes done. */
  setAgent(pane: PaneId, state: AgentState | null, agent = "claude"): void {
    const key = String(pane);
    const before = this.status[key]?.state;
    if (state === null) delete this.status[key];
    else {
      const done = state === "idle" && (before === "working" || before === "done") && !this.seen(pane);
      this.status[key] = { agent, state: done ? "done" : state };
    }
    this.deliverStatus();
  }

  private seen(pane: PaneId): boolean {
    const l = this.model.layout;
    const tab = l.tabs.find((t) => t.id === l.active);
    return !!tab && (tab.zoomed ?? tab.focused) === pane;
  }

  /** After a layout change: the seen pane's done becomes idle. */
  private clearSeen(): void {
    for (const [key, s] of Object.entries(this.status)) {
      if (s.state === "done" && this.seen(Number(key))) {
        this.status[key] = { ...s, state: "idle" };
        this.deliverStatus();
      }
    }
  }

  private deliverStatus(): void {
    const map = structuredClone(this.status);
    queueMicrotask(() => this.onStatus(map));
  }

  /** Where a pane is (null: unknown), as the daemon would report it. */
  setPlace(pane: PaneId, place: PaneDir | null): void {
    if (place) this.places[String(pane)] = place;
    else delete this.places[String(pane)];
    const map = structuredClone(this.places);
    queueMicrotask(() => this.onGit(map));
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
    if (a.t === "close") {
      delete this.status[String(a.pane)];
      this.deliverStatus();
    }
    this.clearSeen();
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
