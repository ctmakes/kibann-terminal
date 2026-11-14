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
import { DEFAULT_FONT_SIZE, FONT_SIZE_MAX, FONT_SIZE_MIN, setFontFamilies } from "../metrics";
import { TerminalView, type TerminalOptions } from "../terminal";
import type { Transport } from "../transport/types";
import type { Layout, LayoutAction, Settings } from "../proto";
import type { LayoutBackend } from "./backend";
import { actionFor, keyFor, menuKeys, setKeybinds, type Action } from "./keymap";
import { HEADER_H, PaneHeader, branchOf, type GitMap } from "./pane";
import { ThemePicker } from "./picker";
import { SettingsModal, type ConfigInfo, type SettingsApi, type SettingsTab, type UpdateStatus } from "./settings";
import { ShortcutSheet, keyText } from "./shortcuts";
import { tooltip } from "./tooltip";
import { FindBar } from "./find";
import { applyChrome, KIBANN_DARK, type ThemeApi } from "./theme";
import type { Theme } from "../proto";
import { Sidebar } from "./sidebar";
import {
  agentName,
  arrivals,
  blockedCount,
  dotFor,
  installStyle,
  makeDot,
  rollUp,
  showDot,
  type Arrival,
  type StatusMap,
} from "./status";
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
  bar: "var(--k-bar)",
  tab: "var(--k-tab)",
  activeTab: "var(--k-tabActive)",
  text: "var(--k-text)",
  activeText: "var(--k-bright)",
  divider: "var(--k-divider)",
};

export interface Pane {
  id: PaneId;
  tab: Tab;
  el: HTMLDivElement;
  /** The agent status dot, in the header. */
  dot: HTMLSpanElement;
  /** Where the pane is (directory or checkout), above the terminal. */
  header: PaneHeader;
  transport: Transport;
  view: TerminalView | null;
  title: string;
  closed: boolean;
  /** Made the first time it's opened. */
  find: FindBar | null;
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
  /** Settings (Cmd+,, the sidebar's gear); without it there's no modal. */
  settings?: SettingsApi;
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
  /** Open the uncommitted changes of `pane`'s checkout (●N was clicked). */
  onDiff?(pane: Pane): void;
  /** Themes to pick from (View → Theme…); without it there's no picker. */
  themes?: ThemeApi;
  /** Say once, on the first launch, where the shortcuts are. */
  firstRunHint?: boolean;
  /** An agent in a pane the user isn't looking at started waiting for them
   * or finished: tell them (a system notification). */
  onNotify?(n: { pane: PaneId; title: string; body: string }): void;
  /** Whether the user is looking at the window (it has focus). */
  windowFocused?(): boolean;
  /** The shortcuts changed (the config's `keybind`): each action's first
   * key as a menu accelerator, by action (JSON); null for none. */
  onKeys?(keys: Record<string, string | null>): void;
}

const HINT_KEY = "kibann.hint.shortcuts";

export class Workspace {
  readonly root: HTMLDivElement;
  readonly sidebar: Sidebar;
  private readonly shortcuts: ShortcutSheet;
  /** Settings, when the app gives it what it needs. */
  readonly settings: SettingsModal | null;
  private readonly picker: ThemePicker | null;
  /** The tab bar's "?" (the shortcut sheet), kept last in the bar. */
  private readonly help: HTMLDivElement;
  private readonly notices: HTMLDivElement;
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
  /** Each pane's directory and checkout, from the daemon. */
  git: GitMap = {};
  private blocked = 0;
  /** No notifications for the first status map: those agents were already
   * waiting or done before the window opened. */
  private statusSeen = false;
  private notifyOn = true;
  /** The config's font size, and the size in use (zoomed from it). */
  private baseFont: number;
  fontSize: number;
  /** Bumped when the font changes, so views made meanwhile catch up. */
  private fontGen = 0;
  /** The config's `keybind` lines in use, as JSON. */
  private keybinds = "[]";

  constructor(
    container: HTMLElement,
    private readonly opts: WorkspaceOptions,
  ) {
    this.platform = opts.platform ?? detectPlatform();
    this.baseFont = this.fontSize = opts.view?.fontSize ?? DEFAULT_FONT_SIZE;
    applyChrome(KIBANN_DARK);
    const sheetKey = () => keyFor({ kind: "shortcuts" }, this.platform);
    this.root = document.createElement("div");
    this.root.className = "workspace";
    Object.assign(this.root.style, {
      position: "absolute",
      inset: "0",
      display: "flex",
      background: COLORS.divider,
      userSelect: "none",
      webkitUserSelect: "none",
    });
    // The sidebar on the left; the tab bar and the panes to its right.
    const main = document.createElement("div");
    main.className = "workspace-main";
    Object.assign(main.style, { flex: "1 1 auto", minWidth: "0", display: "flex", flexDirection: "column" });
    this.sidebar = new Sidebar({
      onPick: (id) => this.show(id),
      onDiff: (id) => {
        const p = this.panes.get(id);
        if (p) this.opts.onDiff?.(p);
      },
      onResize: () => {},
      onShortcuts: () => this.shortcuts.toggle(),
      onThemes: opts.themes ? () => this.run({ kind: "themes" }) : undefined,
      onSettings: opts.settings ? () => this.run({ kind: "settings" }) : undefined,
      settingsKey: () => keyFor({ kind: "settings" }, this.platform),
      shortcutKey: sheetKey,
      sidebarKey: () => keyFor({ kind: "sidebar" }, this.platform),
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
    this.help = document.createElement("div");
    this.help.className = "tab-help";
    this.help.textContent = "?";
    tooltip(this.help, "Keyboard shortcuts", sheetKey);
    Object.assign(this.help.style, {
      flex: "0 0 30px",
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      color: "var(--k-muted)",
      background: "var(--k-tab)",
      cursor: "default",
      fontWeight: "600",
    });
    this.help.addEventListener("mousedown", (e) => {
      e.preventDefault(); // keep focus in the terminal
      this.shortcuts.toggle();
    });
    this.bar.append(this.help);
    // Notices: the first-run hint, a theme that can't be read.
    this.notices = document.createElement("div");
    this.notices.className = "notices";
    Object.assign(this.notices.style, {
      position: "absolute",
      right: "12px",
      bottom: "12px",
      zIndex: "25",
      display: "flex",
      flexDirection: "column",
      gap: "6px",
      alignItems: "flex-end",
      pointerEvents: "none",
      font: "12px -apple-system, system-ui, sans-serif",
    });
    this.stage.append(this.notices);
    main.append(this.bar, this.stage);
    this.shortcuts = new ShortcutSheet(this.platform);
    this.picker = opts.themes
      ? new ThemePicker(opts.themes, () => this.focusedPane?.view?.focus())
      : null;
    this.settings = opts.settings
      ? new SettingsModal(opts.settings, {
          platform: this.platform,
          defaultFontSize: opts.view?.fontSize ?? DEFAULT_FONT_SIZE,
          chooseTheme: () => this.run({ kind: "themes" }),
          onClose: () => this.focusedPane?.view?.focus(),
        })
      : null;
    this.root.append(this.sidebar.el, main, this.shortcuts.el);
    if (this.settings) this.root.append(this.settings.el);
    if (this.picker) this.root.append(this.picker.el);
    if (opts.firstRunHint) this.firstRunHint(sheetKey());
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
    if (action.kind === "shortcuts") {
      this.shortcuts.toggle();
      return;
    }
    if (action.kind === "settings") {
      this.shortcuts.hide();
      this.settings?.toggle();
      return;
    }
    if (action.kind === "themes") {
      this.shortcuts.hide();
      this.settings?.hide();
      void this.picker?.open();
      return;
    }
    if (action.kind === "fontSize") {
      const next = action.step === 0 ? this.baseFont : this.fontSize + action.step;
      this.useFont(Math.min(FONT_SIZE_MAX, Math.max(FONT_SIZE_MIN, next)), false);
      return;
    }
    if (action.kind === "find" && on) {
      this.findBar(on).open();
      return;
    }
    if (action.kind === "findStep" && on) {
      const bar = on.find;
      if (bar?.visible) on.transport.input({ kind: "find_step", older: action.older });
      else this.findBar(on).open();
      return;
    }
    if (action.kind === "sidebar") {
      this.sidebar.toggle();
      this.renderSidebar();
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

  /** Settings, on `tab` (What's New is the Updates tab). */
  openSettings(tab?: SettingsTab): void {
    this.shortcuts.hide();
    this.settings?.show(tab);
  }

  /** Where updating stands, for Settings → Updates. */
  setUpdateStatus(u: UpdateStatus): void {
    this.settings?.setUpdate(u);
  }

  /** The config file's path, problems and themes, for Settings. */
  setConfigInfo(info: ConfigInfo): void {
    this.settings?.setInfo(info);
  }

  /** Show a pane whose agent waits for the user (the menu-bar icon's
   * "N agents are waiting"); false if none does. */
  showWaiting(): boolean {
    for (const [id, s] of Object.entries(this.status)) {
      if (s.state === "blocked" && this.panes.has(Number(id))) {
        this.show(Number(id));
        return true;
      }
    }
    return false;
  }

  /** Show `id`: its tab, focused. */
  show(id: PaneId): void {
    const p = this.panes.get(id);
    if (!p) return;
    if (this.active !== p.tab) this.activate(p.tab);
    this.focusPane(id);
  }

  private sidebarQueued = false;

  /** Redraw the sidebar's cards after the current change settles. */
  private renderSidebar(): void {
    if (this.sidebarQueued) return;
    this.sidebarQueued = true;
    queueMicrotask(() => {
      this.sidebarQueued = false;
      const cards = this.tabs.flatMap((t, i) =>
        panes(t.tree).flatMap((id) => {
          const p = this.panes.get(id);
          if (!p) return [];
          const bg = p.view ? `#${p.view.grid.defaultBg.toString(16).padStart(6, "0")}` : "var(--k-bg)";
          return [
            {
              id,
              title: p.title,
              tab: i,
              shown: t === this.active,
              focused: t.focused === id,
              canvas: p.view?.gridCanvas ?? null,
              bg,
              cursor: () => {
                const g = p.view?.grid;
                return g && g.rows > 0 ? (g.cursor.y + 1) / g.rows : 1;
              },
            },
          ];
        }),
      );
      this.sidebar.render(cards, this.git, this.status);
    });
  }

  private send(a: LayoutAction): void {
    this.opts.backend.act(a).catch((e) => console.warn(`kibann: ${a.t} failed:`, e));
  }

  /** A menu item fired. */
  runFromMenu(action: Action): void {
    this.runOnce(action, "menu");
  }

  private onKeyDown(e: KeyboardEvent): void {
    // The theme picker's search field takes the keys while it's open.
    if (this.picker?.visible) return;
    const action = actionFor(e, this.platform);
    // Settings takes the keys while open (its fields): Esc closes it, and
    // so does its own shortcut. Nothing reaches the panes.
    if (this.settings?.visible) {
      if (e.key === "Escape" || action?.kind === "settings") {
        e.preventDefault();
        e.stopPropagation();
        if (e.key === "Escape") this.settings.hide();
        else this.runOnce(action!, "key");
      }
      return;
    }
    // While the shortcut sheet is open, a key closes it and goes nowhere
    // else (modifiers alone don't, so a shortcut can be pressed to try it).
    if (this.shortcuts.visible && !["Shift", "Control", "Alt", "Meta"].includes(e.key)) {
      e.preventDefault();
      e.stopPropagation();
      // Its own shortcut toggles it closed through runOnce, so the menu
      // item's echo of the same key is skipped instead of reopening it.
      if (action?.kind !== "shortcuts") this.shortcuts.hide();
      if (action) this.runOnce(action, "key");
      return;
    }
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
      for (const id of panes(t.tree)) this.setPaneActive(id, id === t.focused);
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
    const prev = this.status;
    this.status = map;
    if (this.statusSeen && this.notifyOn && this.opts.onNotify) {
      const focused = this.opts.windowFocused?.() ?? document.hasFocus();
      const looking = focused ? (this.focusedPane?.id ?? null) : null;
      for (const a of arrivals(prev, map, looking)) this.notify(a);
    }
    this.statusSeen = true;
    for (const p of this.panes.values()) this.showPaneDot(p);
    this.updateBar();
    const blocked = blockedCount(map);
    if (blocked !== this.blocked) {
      this.blocked = blocked;
      this.opts.onBlocked?.(blocked);
    }
  }

  private notify(a: Arrival): void {
    const p = this.panes.get(a.pane);
    if (!p) return;
    const who = agentName(a.agent);
    const title = a.state === "blocked" ? `${who} is waiting for you` : `${who} finished`;
    const place = this.git[String(a.pane)];
    const where = place?.git ? [place.git.repo, place.git.branch].filter(Boolean).join(" · ") : place?.shown;
    const body = [where, p.title].filter(Boolean).join(" — ");
    this.opts.onNotify?.({ pane: a.pane, title, body });
  }

  /** The config file's settings: fonts and notifications. */
  setSettings(s: Settings): void {
    this.settings?.setSettings(s);
    this.sidebar.setPreviews(s.sidebar_preview);
    this.notifyOn = s.agent_notifications;
    const keys = JSON.stringify(s.keybind);
    if (keys !== this.keybinds) {
      this.keybinds = keys;
      for (const e of setKeybinds(this.platform, s.keybind)) this.notice(`Config: ${e}`, 8000);
      this.shortcuts.rebuild();
      this.opts.onKeys?.(menuKeys(this.platform));
    }
    const families = setFontFamilies(s.font_family);
    const base = s.font_size ?? this.opts.view?.fontSize ?? DEFAULT_FONT_SIZE;
    // A new size in the file replaces any zoom.
    const size = base !== this.baseFont ? base : this.fontSize;
    this.baseFont = base;
    this.useFont(size, families);
  }

  /** Use `size` in every pane (and new ones); `force` after the families
   * changed, when the size may not have. */
  private useFont(size: number, force: boolean): void {
    if (size === this.fontSize && !force) return;
    this.fontSize = size;
    this.fontGen++;
    for (const p of this.panes.values()) void p.view?.setFont(size);
  }

  /** Show the daemon's map of directories and checkouts. */
  setGit(map: GitMap): void {
    this.git = map;
    for (const p of this.panes.values()) p.header.show(map[String(p.id)]);
    this.updateBar();
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
    // Its number: Cmd+1–8 goes to it (Ctrl elsewhere).
    const n = document.createElement("span");
    n.className = "tab-number";
    Object.assign(n.style, { position: "absolute", right: "10px", fontSize: "11px", opacity: "0.55", fontVariantNumeric: "tabular-nums" });
    b.append(x, tab.dot, label, n);
    b.addEventListener("mousedown", (e) => {
      e.preventDefault(); // keep focus in the terminal
      if (e.button === 1 || e.target === x) {
        for (const id of panes(tab.tree)) this.send({ t: "close", pane: id });
      } else if (e.button === 0) this.activate(tab);
    });
    this.bar.append(b);
  }

  private updateBar(): void {
    this.renderSidebar();
    this.bar.style.display = this.tabs.length > 1 ? "flex" : "none";
    this.bar.append(this.help); // after the tabs, wherever they moved
    this.tabs.forEach((t, i) => {
      const on = t === this.active;
      t.button.style.background = on ? COLORS.activeTab : COLORS.tab;
      t.button.style.color = on ? COLORS.activeText : COLORS.text;
      const branch = branchOf(this.git[String(t.focused)]);
      const title = (this.panes.get(t.focused)?.title || "shell") + (branch ? ` · ${branch}` : "");
      t.button.querySelector<HTMLSpanElement>(".tab-title")!.textContent = title;
      t.button.title = title;
      t.button.querySelector<HTMLSpanElement>(".tab-number")!.textContent = String(i + 1);
      showDot(t.dot, rollUp(this.status, panes(t.tree)));
    });
  }

  /** Color the window around the terminals for `theme`. */
  setTheme(theme: Theme): void {
    applyChrome(theme);
  }

  /** Show `text` in the corner for a while. */
  notice(text: string, ms = 8000, action?: { label: string; run(): void }): void {
    const n = document.createElement("div");
    n.className = "notice";
    n.textContent = text;
    if (action) {
      // A notice with a button stays until it's used or dismissed.
      const b = document.createElement("button");
      b.className = "notice-action";
      b.textContent = action.label;
      Object.assign(b.style, {
        marginLeft: "10px",
        padding: "2px 10px",
        border: "1px solid var(--k-divider)",
        borderRadius: "5px",
        background: "var(--k-current)",
        color: "var(--k-current-text)",
        font: "12px -apple-system, system-ui, sans-serif",
      });
      b.addEventListener("click", () => {
        n.remove();
        action.run();
      });
      const x = document.createElement("button");
      x.className = "notice-close";
      x.textContent = "✕";
      x.setAttribute("aria-label", "Dismiss");
      Object.assign(x.style, { marginLeft: "6px", border: "0", background: "transparent", color: "var(--k-muted)" });
      x.addEventListener("click", () => n.remove());
      n.append(b, x);
      n.style.pointerEvents = "auto";
    }
    Object.assign(n.style, {
      maxWidth: "420px",
      padding: "7px 10px",
      borderRadius: "6px",
      background: "var(--k-panel)",
      border: "1px solid var(--k-divider)",
      color: "var(--k-strong)",
      boxShadow: "0 4px 16px var(--k-shadow)",
    });
    this.notices.append(n);
    if (!action) window.setTimeout(() => n.remove(), ms);
  }

  private firstRunHint(key: string): void {
    try {
      if (localStorage.getItem(HINT_KEY)) return;
      localStorage.setItem(HINT_KEY, "1");
    } catch {
      return; // can't remember it was shown: don't show it every time
    }
    this.notice(`Press ${key} to see every keyboard shortcut.`, 10000);
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
    const dot = makeDot();
    const header = new PaneHeader(dot, () => {
      const p = this.panes.get(id);
      if (p) this.opts.onDiff?.(p);
    });
    header.show(this.git[String(id)]);
    header.setActive(tab.focused === id);
    // The terminal fills what the header leaves.
    const body = document.createElement("div");
    body.className = "pane-body";
    Object.assign(body.style, { position: "absolute", left: "0", right: "0", top: `${HEADER_H}px`, bottom: "0" });
    el.append(header.el, body);
    tab.el.append(el);
    const pane: Pane = { id, tab, el, dot, header, transport, view: null, title: "", closed: false, find: null };
    this.panes.set(id, pane);
    this.showPaneDot(pane);
    const gen = this.fontGen;
    void TerminalView.create(body, transport, {
      ...this.opts.view,
      fontSize: this.fontSize,
      platform: this.platform,
      autofocus: false,
      onFocusChange: (focused) => focused && this.onPaneFocused(pane),
      onPaint: (info) => {
        this.opts.view?.onPaint?.(info);
        this.sidebar.painted(id);
      },
      onTitle: (title) => {
        pane.title = title;
        this.updateBar();
        if (pane === this.focusedPane) this.emitTitle();
      },
      onFind: (find) => pane.find?.show(find),
      onContextMenu: this.opts.onContextMenu ? (e) => this.opts.onContextMenu!(pane, e) : undefined,
    }).then((view) => {
      if (pane.closed) {
        view.dispose();
        return;
      }
      pane.view = view;
      if (gen !== this.fontGen) void view.setFont(this.fontSize);
      this.renderSidebar();
      view.setActive(pane.tab.focused === pane.id);
      if (this.active === pane.tab && pane.tab.focused === pane.id) view.focus();
    });
    return pane;
  }

  /** The pane's find bar, made on first use. */
  private findBar(pane: Pane): FindBar {
    if (pane.find) return pane.find;
    const bar = new FindBar({
      onNeedle: (needle) => pane.transport.input({ kind: "find", needle }),
      onStep: (older) => pane.transport.input({ kind: "find_step", older }),
      onClose: () => {
        pane.transport.input({ kind: "find_end" });
        if (pane === this.focusedPane) pane.view?.focus();
      },
    });
    bar.show(pane.view?.grid.find ?? null);
    pane.el.querySelector(".pane-body")?.append(bar.el);
    pane.find = bar;
    return bar;
  }

  private setPaneActive(id: PaneId, active: boolean): void {
    const p = this.panes.get(id);
    p?.view?.setActive(active);
    p?.header.setActive(active);
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
    for (const id of panes(tab.tree)) this.setPaneActive(id, id === tab.focused);
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
    this.sidebar.dispose();
    this.root.remove();
  }
}
