// Tabs of split panes, each pane a TerminalView with its own transport.
//
// The layout belongs to the backend (the daemon): user actions go to it, and
// the workspace changes when a layout comes back (`apply`). Only focus moves
// at once, so typing never waits on a round trip.
//
// Panes are absolutely positioned from `layout.rects`, so a resize is one
// pass and dividers are plain elements on top. Inactive tabs stay in the DOM,
// hidden, so their terminals keep their state and frames.

import { detectPlatform, type Platform } from "../input/keyboard";
import { TerminalView, type TerminalOptions } from "../terminal";
import type { Transport } from "../transport/types";
import type { Layout, LayoutAction } from "../proto";
import type { LayoutBackend } from "./backend";
import { actionFor, type Action } from "./keymap";
import { blockedCount, dotFor, installStyle, makeDot, rollUp, showDot, type StatusMap } from "./status";
import {
  cycle,
  dividers,
  neighbor,
  panes,
  ratioAt,
  rects,
  resizeTarget,
  setRatio,
  type PaneId,
  type Rect,
  type Tree,
} from "./layout";

/** How far one resize shortcut moves a divider, in CSS px (Ghostty's 10). */
const RESIZE_STEP = 10;
/** A menu item firing right before or after the same shortcut is that shortcut. */
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
  /** The agent status dot in the pane's corner. */
  dot: HTMLSpanElement;
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
  dot: HTMLSpanElement;
}

export interface WorkspaceOptions {
  /** Where layout actions go. */
  backend: LayoutBackend;
  /** A transport for the pane (session) `pane`. */
  transport(pane: PaneId): Transport;
  platform?: Platform;
  /** Passed to every TerminalView. */
  view?: Pick<TerminalOptions, "fontSize" | "dpr" | "onPaint" | "onOpenLink">;
  /** The window's title should change (the active tab's title). */
  onTitle?(title: string): void;
  /** The last pane closed. */
  onEmpty?(): void;
  /** Right-click in a pane with no program taking the mouse. */
  onContextMenu?(pane: Pane, e: MouseEvent): void;
  /** How many panes' agents wait for the user changed (the Dock badge). */
  onBlocked?(count: number): void;
}

export class Workspace {
  readonly root: HTMLDivElement;
  readonly bar: HTMLDivElement;
  readonly stage: HTMLDivElement;
  readonly tabs: Tab[] = [];
  readonly panes = new Map<PaneId, Pane>();
  active: Tab | null = null;
  private readonly platform: Platform;
  private lastRun: { action: string; from: "key" | "menu"; at: number } | null = null;
  private readonly cleanups: Array<() => void> = [];
  /** Agent status by pane, from the daemon. */
  status: StatusMap = {};
  private blocked = 0;

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

    installStyle();
    // Pulsing dots don't animate in a hidden window.
    const onVisible = () => this.root.classList.toggle("kibann-hidden", document.hidden);
    document.addEventListener("visibilitychange", onVisible);
    this.cleanups.push(() => document.removeEventListener("visibilitychange", onVisible));

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
        this.send({ t: "new_tab", near: on?.id, focus: true });
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
        this.send({ t: "close", pane: on.id });
        return;
      case "split":
        this.send({ t: "split", pane: on.id, dir: action.dir, focus: true });
        return;
      case "focusCycle":
        this.focusPane(cycle(tab.tree, on.id, action.step));
        return;
      case "focus": {
        const next = neighbor(tab.tree, this.box(), on.id, action.toward);
        if (next !== null) this.focusPane(next);
        return;
      }
      case "resize": {
        const r = resizeTarget(tab.tree, this.box(), on.id, action.toward, RESIZE_STEP);
        if (r) this.send({ t: "set_ratio", tab: tab.id, path: r.path, ratio: r.ratio });
        return;
      }
      case "equalize":
        this.send({ t: "equalize", tab: tab.id });
        return;
      case "zoom": {
        const zoom = tab.zoomed === null && panes(tab.tree).length > 1;
        this.send({ t: "zoom", tab: tab.id, pane: zoom ? on.id : undefined });
        return;
      }
    }
  }

  private send(a: LayoutAction): void {
    this.opts.backend.act(a).catch((e) => console.warn(`kibann: ${a.t} failed:`, e));
  }

  /** A menu item fired. */
  runFromMenu(action: Action): void {
    this.runOnce(action, "menu");
  }

  private onKeyDown(e: KeyboardEvent): void {
    const action = actionFor(e, this.platform);
    if (!action) return;
    e.preventDefault();
    e.stopPropagation();
    this.runOnce(action, "key");
  }

  /**
   * A shortcut can arrive twice, as a key press in the page and as its menu
   * item firing, in either order. The second is skipped: running a toggle
   * (zoom) twice would undo it.
   */
  private runOnce(action: Action, from: "key" | "menu"): void {
    const key = JSON.stringify(action);
    const last = this.lastRun;
    if (last && last.action === key && last.from !== from && performance.now() - last.at < MENU_ECHO_MS) {
      this.lastRun = null;
      return;
    }
    this.lastRun = { action: key, from, at: performance.now() };
    this.run(action);
  }

  // ---- the layout from the backend ----

  /**
   * Show `layout`: add and remove tabs and panes to match, then place them.
   * Keyboard focus moves only when the focused pane or the shown tab
   * changed, so a pane an agent opens in the background doesn't take it.
   */
  apply(layout: Layout): void {
    const hadTabs = this.tabs.length > 0;
    const before = this.active ? { tab: this.active.id, pane: this.active.focused } : null;
    const byId = new Map(this.tabs.map((t) => [t.id, t]));
    const next: Tab[] = layout.tabs.map((lt) => {
      const tab = byId.get(lt.id) ?? this.makeTab(lt.id, lt.tree, lt.focused);
      tab.tree = lt.tree;
      tab.focused = lt.focused;
      tab.zoomed = lt.zoomed;
      return tab;
    });
    for (const t of this.tabs) {
      if (next.includes(t)) continue;
      t.el.remove();
      t.button.remove();
    }
    this.tabs.splice(0, this.tabs.length, ...next);
    for (const t of next) this.bar.append(t.button); // in layout order

    const want = new Map<PaneId, Tab>();
    for (const t of next) for (const id of panes(t.tree)) want.set(id, t);
    for (const [id, p] of this.panes) if (!want.has(id)) this.dropPane(p);
    for (const [id, tab] of want) {
      const p = this.panes.get(id);
      if (!p) this.addPane(tab, id, this.opts.transport(id));
      else if (p.tab !== tab) {
        p.tab = tab;
        tab.el.append(p.el);
      }
    }

    const active = next.find((t) => t.id === layout.active) ?? next[0] ?? null;
    if (this.active && this.active !== active) this.active.el.style.display = "none";
    this.active = active;
    for (const t of next) {
      if (t === active) {
        t.el.style.display = "block";
        this.layout(t);
      } else this.place(t);
      for (const id of panes(t.tree)) this.panes.get(id)?.view?.setActive(id === t.focused);
    }
    if (active && (before?.tab !== active.id || before.pane !== active.focused)) {
      this.panes.get(active.focused)?.view?.focus();
    }
    this.updateBar();
    this.emitTitle();
    if (hadTabs && next.length === 0) this.opts.onEmpty?.();
  }

  /** Show the daemon's agent status: pane dots, tab dots, the badge. */
  setStatus(map: StatusMap): void {
    this.status = map;
    for (const p of this.panes.values()) this.showPaneDot(p);
    this.updateBar();
    const blocked = blockedCount(map);
    if (blocked !== this.blocked) {
      this.blocked = blocked;
      this.opts.onBlocked?.(blocked);
    }
  }

  private showPaneDot(p: Pane): void {
    showDot(p.dot, dotFor(this.status, p.id), this.status[String(p.id)]?.agent);
  }

  private makeTab(id: number, tree: Tree, focused: PaneId): Tab {
    const el = document.createElement("div");
    el.className = "tab-stage";
    Object.assign(el.style, { position: "absolute", inset: "0", display: "none" });
    this.stage.append(el);
    const button = document.createElement("div");
    const dot = makeDot({ marginRight: "6px" });
    const tab: Tab = { id, tree, focused, zoomed: null, el, button, dot };
    this.makeTabButton(tab);
    return tab;
  }

  // ---- tabs ----

  /** Show `tab` now, and tell the backend. */
  activate(tab: Tab): void {
    if (this.active !== tab) {
      if (this.active) this.active.el.style.display = "none";
      this.active = tab;
      tab.el.style.display = "block";
      this.layout(tab);
      this.updateBar();
      this.send({ t: "activate", tab: tab.id });
    }
    this.focusPane(tab.focused);
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
    b.append(x, tab.dot, label);
    b.addEventListener("mousedown", (e) => {
      e.preventDefault(); // keep focus in the terminal
      if (e.button === 1 || e.target === x) {
        for (const id of panes(tab.tree)) this.send({ t: "close", pane: id });
      } else if (e.button === 0) this.activate(tab);
    });
    this.bar.append(b);
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
      showDot(t.dot, rollUp(this.status, panes(t.tree)));
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
    // Above the canvas, clear of the scrollbar.
    const dot = makeDot({ position: "absolute", top: "6px", right: "14px", zIndex: "3" });
    el.append(dot);
    tab.el.append(el);
    const pane: Pane = { id, tab, el, dot, transport, view: null, title: "", closed: false };
    this.panes.set(id, pane);
    this.showPaneDot(pane);
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

  /** The pane left the layout (closed, or its program exited). */
  private dropPane(pane: Pane): void {
    pane.closed = true;
    this.panes.delete(pane.id);
    pane.view?.dispose();
    pane.el.remove();
    pane.transport.close?.();
  }

  /** Focus a pane now (the backend is told; same rules as the daemon). */
  focusPane(id: PaneId): void {
    const pane = this.panes.get(id);
    if (!pane) return;
    const tab = pane.tab;
    if (tab.zoomed !== null && tab.zoomed !== id) {
      tab.zoomed = null;
      this.layout(tab);
    }
    if (tab.focused !== id) {
      tab.focused = id;
      this.send({ t: "focus", pane: id });
    }
    this.updateActive(tab);
    if (tab === this.active) pane.view?.focus();
  }

  private onPaneFocused(pane: Pane): void {
    const tab = pane.tab;
    if (tab.focused === pane.id) return;
    tab.focused = pane.id;
    this.send({ t: "focus", pane: pane.id });
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
        let ratio: number | null = null;
        const move = (ev: PointerEvent) => {
          ratio = ratioAt(d, ev.clientX - origin.left, ev.clientY - origin.top);
          tab.tree = setRatio(tab.tree, d.path, ratio);
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
          if (ratio !== null) this.send({ t: "set_ratio", tab: tab.id, path: d.path, ratio });
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
