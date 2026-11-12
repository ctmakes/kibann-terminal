// Tabs of split panes, each pane a TerminalView with its own transport.
//
// Panes are absolutely positioned from `layout.rects`, so a resize is one
// pass and dividers are plain elements on top. Inactive tabs stay in the DOM,
// hidden, so their terminals keep their state and frames.

import { detectPlatform, type Platform } from "../input/keyboard";
import { TerminalView, type TerminalOptions } from "../terminal";
import type { Transport } from "../transport/types";
import { actionFor, type Action } from "./keymap";
import {
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
  setRatio,
  split,
  type PaneId,
  type Rect,
  type Tree,
} from "./layout";

/** How far one resize shortcut moves a divider, in CSS px (Ghostty's 10). */
const RESIZE_STEP = 10;
/** A menu item firing right after the same shortcut was handled is that shortcut. */
const MENU_ECHO_MS = 300;

const COLORS = {
  bar: "#111111",
  tab: "#1d1d1d",
  activeTab: "#2a2a2a",
  text: "#9a9a9a",
  activeText: "#e6e6e6",
  divider: "#3a3a3a",
};

export interface Pane {
  id: PaneId;
  tab: Tab;
  el: HTMLDivElement;
  transport: Transport;
  view: TerminalView | null;
  title: string;
  closed: boolean;
}

export interface Tab {
  id: number;
  tree: Tree;
  focused: PaneId;
  zoomed: PaneId | null;
  el: HTMLDivElement;
  button: HTMLDivElement;
}

export interface WorkspaceOptions {
  /** A transport for a new pane, opened from `near` (for its directory). */
  transport(near: Transport | null): Transport;
  platform?: Platform;
  /** Passed to every TerminalView. */
  view?: Pick<TerminalOptions, "fontSize" | "dpr" | "onPaint">;
  /** The window's title should change (the active tab's title). */
  onTitle?(title: string): void;
  /** The last pane closed. */
  onEmpty?(): void;
  /** Right-click in a pane with no program taking the mouse. */
  onContextMenu?(pane: Pane, e: MouseEvent): void;
}

export class Workspace {
  readonly root: HTMLDivElement;
  readonly bar: HTMLDivElement;
  readonly stage: HTMLDivElement;
  readonly tabs: Tab[] = [];
  readonly panes = new Map<PaneId, Pane>();
  active: Tab | null = null;
  private nextPane = 1;
  private nextTab = 1;
  private readonly platform: Platform;
  private lastKey: { action: string; at: number } | null = null;
  private readonly cleanups: Array<() => void> = [];

  constructor(
    container: HTMLElement,
    private readonly opts: WorkspaceOptions,
  ) {
    this.platform = opts.platform ?? detectPlatform();
    this.root = document.createElement("div");
    this.root.className = "workspace";
    Object.assign(this.root.style, {
      position: "absolute",
      inset: "0",
      display: "flex",
      flexDirection: "column",
      background: COLORS.divider,
      userSelect: "none",
      webkitUserSelect: "none",
    });
    this.bar = document.createElement("div");
    this.bar.className = "tab-bar";
    Object.assign(this.bar.style, {
      display: "none",
      flex: "0 0 30px",
      background: COLORS.bar,
      gap: "1px",
      font: "12px -apple-system, system-ui, sans-serif",
    });
    this.stage = document.createElement("div");
    this.stage.className = "stage";
    Object.assign(this.stage.style, { position: "relative", flex: "1 1 auto", overflow: "hidden" });
    this.root.append(this.bar, this.stage);
    container.append(this.root);

    const ro = new ResizeObserver(() => this.active && this.layout(this.active));
    ro.observe(this.stage);
    const onKey = (e: KeyboardEvent) => this.onKeyDown(e);
    window.addEventListener("keydown", onKey, { capture: true });
    this.cleanups.push(
      () => ro.disconnect(),
      () => window.removeEventListener("keydown", onKey, { capture: true }),
    );
  }

  get focusedPane(): Pane | null {
    return this.active ? (this.panes.get(this.active.focused) ?? null) : null;
  }

  // ---- actions ----

  /** Run an action from a shortcut, a menu or the right-click menu. */
  run(action: Action, on: Pane | null = this.focusedPane): void {
    const tab = on?.tab ?? this.active;
    switch (action.kind) {
      case "newTab":
        this.newTab(this.opts.transport(on?.transport ?? null));
        return;
      case "goToTab": {
        const t = action.index < 0 ? this.tabs[this.tabs.length - 1] : this.tabs[action.index];
        if (t) this.activate(t);
        return;
      }
      case "tabCycle":
        if (this.active && this.tabs.length > 1) {
          const i = this.tabs.indexOf(this.active);
          this.activate(this.tabs[(i + action.step + this.tabs.length) % this.tabs.length]);
        }
        return;
    }
    if (!on || !tab) return;
    switch (action.kind) {
      case "close":
        this.closePane(on);
        return;
      case "split":
        this.splitPane(on, action.dir);
        return;
      case "focusCycle":
        this.focusPane(cycle(tab.tree, on.id, action.step));
        return;
      case "focus": {
        const next = neighbor(tab.tree, this.box(), on.id, action.toward);
        if (next !== null) this.focusPane(next);
        return;
      }
      case "resize":
        tab.zoomed = null;
        tab.tree = resize(tab.tree, this.box(), on.id, action.toward, RESIZE_STEP);
        this.layout(tab);
        return;
      case "equalize":
        tab.tree = equalize(tab.tree);
        this.layout(tab);
        return;
      case "zoom":
        tab.zoomed = tab.zoomed === null && panes(tab.tree).length > 1 ? on.id : null;
        this.layout(tab);
        this.focusPane(on.id);
        return;
    }
  }

  /** A menu item fired. Skips the echo of a shortcut the page already handled. */
  runFromMenu(action: Action): void {
    const key = JSON.stringify(action);
    if (this.lastKey && this.lastKey.action === key && performance.now() - this.lastKey.at < MENU_ECHO_MS) {
      this.lastKey = null;
      return;
    }
    this.run(action);
  }

  private onKeyDown(e: KeyboardEvent): void {
    const action = actionFor(e, this.platform);
    if (!action) return;
    e.preventDefault();
    e.stopPropagation();
    this.lastKey = { action: JSON.stringify(action), at: performance.now() };
    this.run(action);
  }

  // ---- tabs ----

  newTab(transport: Transport): Tab {
    const el = document.createElement("div");
    el.className = "tab-stage";
    Object.assign(el.style, { position: "absolute", inset: "0", display: "none" });
    this.stage.append(el);
    const button = document.createElement("div");
    const id = this.nextTab++;
    const paneId = this.nextPane++;
    const tab: Tab = { id, tree: leaf(paneId), focused: paneId, zoomed: null, el, button };
    this.makeTabButton(tab);
    this.tabs.push(tab);
    this.addPane(tab, paneId, transport);
    this.activate(tab);
    this.updateBar();
    return tab;
  }

  activate(tab: Tab): void {
    if (this.active === tab) {
      this.focusPane(tab.focused);
      return;
    }
    if (this.active) this.active.el.style.display = "none";
    this.active = tab;
    tab.el.style.display = "block";
    this.layout(tab);
    this.focusPane(tab.focused);
    this.updateBar();
    this.emitTitle();
  }

  private closeTab(tab: Tab): void {
    const i = this.tabs.indexOf(tab);
    this.tabs.splice(i, 1);
    tab.el.remove();
    tab.button.remove();
    if (this.active === tab) {
      this.active = null;
      const next = this.tabs[Math.min(i, this.tabs.length - 1)];
      if (next) this.activate(next);
    }
    this.updateBar();
    if (this.tabs.length === 0) this.opts.onEmpty?.();
  }

  private makeTabButton(tab: Tab): void {
    const b = tab.button;
    b.className = "tab";
    Object.assign(b.style, {
      flex: "1 1 0",
      minWidth: "0",
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      position: "relative",
      padding: "0 28px",
      cursor: "default",
    });
    const label = document.createElement("span");
    label.className = "tab-title";
    Object.assign(label.style, { overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" });
    const x = document.createElement("span");
    x.className = "tab-close";
    x.textContent = "×";
    Object.assign(x.style, { position: "absolute", left: "8px", fontSize: "14px", opacity: "0.6" });
    b.append(x, label);
    b.addEventListener("mousedown", (e) => {
      e.preventDefault(); // keep focus in the terminal
      if (e.button === 1 || e.target === x) this.closeTabPanes(tab);
      else if (e.button === 0) this.activate(tab);
    });
    this.bar.append(b);
  }

  private closeTabPanes(tab: Tab): void {
    for (const id of panes(tab.tree)) {
      const p = this.panes.get(id);
      if (p) this.closePane(p);
    }
  }

  private updateBar(): void {
    this.bar.style.display = this.tabs.length > 1 ? "flex" : "none";
    for (const t of this.tabs) {
      const on = t === this.active;
      t.button.style.background = on ? COLORS.activeTab : COLORS.tab;
      t.button.style.color = on ? COLORS.activeText : COLORS.text;
      const title = this.panes.get(t.focused)?.title || "shell";
      t.button.querySelector<HTMLSpanElement>(".tab-title")!.textContent = title;
      t.button.title = title;
    }
  }

  private emitTitle(): void {
    const p = this.focusedPane;
    this.opts.onTitle?.(p?.title || "Kibann Terminal");
  }

  // ---- panes ----

  private addPane(tab: Tab, id: PaneId, transport: Transport): Pane {
    const el = document.createElement("div");
    el.className = "pane";
    el.dataset.pane = String(id);
    Object.assign(el.style, { position: "absolute", overflow: "hidden" });
    tab.el.append(el);
    const pane: Pane = { id, tab, el, transport, view: null, title: "", closed: false };
    this.panes.set(id, pane);
    transport.onExit = () => this.closePane(pane);
    if (this.active === tab) this.layout(tab);
    else this.place(tab);
    void TerminalView.create(el, transport, {
      ...this.opts.view,
      platform: this.platform,
      autofocus: false,
      onFocusChange: (focused) => focused && this.onPaneFocused(pane),
      onTitle: (title) => {
        pane.title = title;
        this.updateBar();
        if (pane === this.focusedPane) this.emitTitle();
      },
      onContextMenu: this.opts.onContextMenu ? (e) => this.opts.onContextMenu!(pane, e) : undefined,
    }).then((view) => {
      if (pane.closed) {
        view.dispose();
        return;
      }
      pane.view = view;
      view.setActive(pane.tab.focused === pane.id);
      if (this.active === pane.tab && pane.tab.focused === pane.id) view.focus();
    });
    return pane;
  }

  splitPane(on: Pane, dir: "row" | "column"): Pane {
    const tab = on.tab;
    tab.zoomed = null;
    const id = this.nextPane++;
    tab.tree = split(tab.tree, on.id, dir, id);
    tab.focused = id;
    const pane = this.addPane(tab, id, this.opts.transport(on.transport));
    this.layout(tab);
    this.updateActive(tab);
    return pane;
  }

  closePane(pane: Pane): void {
    if (pane.closed) return;
    pane.closed = true;
    const tab = pane.tab;
    const order = panes(tab.tree);
    const i = order.indexOf(pane.id);
    this.panes.delete(pane.id);
    pane.view?.dispose();
    pane.el.remove();
    pane.transport.close?.();
    const tree = remove(tab.tree, pane.id);
    if (!tree) {
      this.closeTab(tab);
      return;
    }
    tab.tree = tree;
    if (tab.zoomed === pane.id) tab.zoomed = null;
    if (tab.focused === pane.id) {
      const left = panes(tree);
      tab.focused = left[Math.max(0, i - 1)] ?? left[0];
    }
    this.layout(tab);
    if (tab === this.active) this.focusPane(tab.focused);
    this.updateBar();
  }

  focusPane(id: PaneId): void {
    const pane = this.panes.get(id);
    if (!pane) return;
    const tab = pane.tab;
    if (tab.zoomed !== null && tab.zoomed !== id) {
      tab.zoomed = null;
      this.layout(tab);
    }
    tab.focused = id;
    this.updateActive(tab);
    if (tab === this.active) pane.view?.focus();
  }

  private onPaneFocused(pane: Pane): void {
    const tab = pane.tab;
    if (tab.focused === pane.id) return;
    tab.focused = pane.id;
    this.updateActive(tab);
  }

  private updateActive(tab: Tab): void {
    for (const id of panes(tab.tree)) this.panes.get(id)?.view?.setActive(id === tab.focused);
    if (tab === this.active) {
      this.updateBar();
      this.emitTitle();
    }
  }

  // ---- layout ----

  private box(): Rect {
    return { x: 0, y: 0, w: this.stage.clientWidth, h: this.stage.clientHeight };
  }

  /** Position a tab's panes and dividers in the stage. */
  layout(tab: Tab): void {
    this.place(tab);
    for (const d of tab.el.querySelectorAll(".divider")) d.remove();
    if (tab.zoomed !== null) return;
    for (const d of dividers(tab.tree, this.box())) {
      const el = document.createElement("div");
      el.className = "divider";
      const grab = 6;
      const row = d.dir === "row";
      Object.assign(el.style, {
        position: "absolute",
        left: `${row ? d.rect.x - grab / 2 : d.rect.x}px`,
        top: `${row ? d.rect.y : d.rect.y - grab / 2}px`,
        width: `${row ? d.rect.w + grab : d.rect.w}px`,
        height: `${row ? d.rect.h : d.rect.h + grab}px`,
        cursor: row ? "col-resize" : "row-resize",
        zIndex: "2",
      });
      el.addEventListener("pointerdown", (e) => {
        e.preventDefault();
        el.setPointerCapture(e.pointerId);
        const origin = this.stage.getBoundingClientRect();
        const move = (ev: PointerEvent) => {
          tab.tree = setRatio(tab.tree, d.path, ratioAt(d, ev.clientX - origin.left, ev.clientY - origin.top));
          this.place(tab);
          // Keep the grab area under the pointer while dragging.
          const next = dividers(tab.tree, this.box()).find((n) => n.path.join() === d.path.join());
          if (next) {
            el.style.left = `${row ? next.rect.x - grab / 2 : next.rect.x}px`;
            el.style.top = `${row ? next.rect.y : next.rect.y - grab / 2}px`;
          }
        };
        const up = () => {
          el.removeEventListener("pointermove", move);
          el.removeEventListener("pointerup", up);
          this.layout(tab);
          this.focusPane(tab.focused);
        };
        el.addEventListener("pointermove", move);
        el.addEventListener("pointerup", up);
      });
      tab.el.append(el);
    }
  }

  /** Position the panes only. */
  private place(tab: Tab): void {
    const box = this.box();
    const all = tab.zoomed !== null ? new Map([[tab.zoomed, box]]) : rects(tab.tree, box);
    for (const id of panes(tab.tree)) {
      const p = this.panes.get(id);
      if (!p) continue;
      const r = all.get(id);
      if (!r) {
        p.el.style.display = "none";
        continue;
      }
      Object.assign(p.el.style, {
        display: "block",
        left: `${r.x}px`,
        top: `${r.y}px`,
        width: `${r.w}px`,
        height: `${r.h}px`,
      });
    }
  }

  dispose(): void {
    for (const p of [...this.panes.values()]) {
      p.closed = true;
      p.view?.dispose();
      p.transport.close?.();
    }
    for (const c of this.cleanups) c();
    this.root.remove();
  }
}
