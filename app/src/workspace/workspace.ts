// Tabs of split panes, each pane a TerminalView with its own transport.
//
// The layout belongs to the backend (the daemon): user actions go to it, and
// the workspace changes when a layout comes back (`apply`). Only focus moves
// at once, so typing never waits on a round trip.
//
// Views of their own: the tab bar (tabbar.ts), the stage's panes and
// dividers (stage.ts), notices (notices.ts), the sidebar, and what parts
// see and may do (bridge.ts). Inactive tabs stay in the DOM, hidden, so
// their terminals keep their state and frames.

import { detectPlatform, type Platform } from "../input/keyboard";
import { DEFAULT_FONT_SIZE, FONT_SIZE_MAX, FONT_SIZE_MIN, setFontFamilies } from "../metrics";
import { TerminalView, type TerminalOptions } from "../terminal";
import { cursorAt } from "../painter/thumbnail";
import type { Transport } from "../transport/types";
import type { Layout, LayoutAction, Settings } from "../proto";
import type { LayoutBackend } from "./backend";
import { actionFor, keyFor, menuKeys, setKeybinds, type Action } from "./keymap";
import { HEADER_H, PaneHeader, branchOf, type GitMap } from "./pane";
import { ThemePicker } from "./picker";
import { WorkspacePicker, type WorkspaceApi } from "./workspaces";
import { SettingsModal, type ConfigInfo, type SettingsApi, type SettingsTab, type UpdateStatus } from "./settings";
import { ShortcutSheet, keyText } from "./shortcuts";
import { FindBar } from "./find";
import { applyChrome, KIBANN_DARK, type ThemeApi } from "./theme";
import type { ScreenNow, Theme } from "../proto";
import { ScreenView, type BlockSlot } from "../screen/engine";
import { PartHost } from "../screen/parts";
import { partState, partTheme, runPartAction, tabName, type PartActions } from "./bridge";
import { paneThumbnail } from "../painter/thumbnail";
import { ScreenPicker, type ScreenApi } from "../screen/picker";
import { DEFAULT_SCREEN } from "../screen/default";
import { Sidebar } from "./sidebar";
import { makeDividers, paneRects, place } from "./stage";
import { Notices } from "./notices";
import { TabBar, type TabButton } from "./tabbar";
import {
  agentName,
  arrivals,
  blockedCount,
  dotFor,
  makeDot,
  rollUp,
  showDot,
  type Arrival,
  type StatusMap,
} from "./status";
import {
  cycle,
  neighbor,
  panes,
  resizeTarget,
  setRatio,
  type PaneId,
  type Rect,
  type Tree,
} from "./layout";
import "../ui.css";
import "./workspace.css";

/** How far one resize shortcut moves a divider, in CSS px (Ghostty's 10). */
const RESIZE_STEP = 10;

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
  /** Its name, from a workspace file; else the focused pane's title shows. */
  title: string | null;
  tree: Tree;
  focused: PaneId;
  zoomed: PaneId | null;
  el: HTMLDivElement;
  button: TabButton;
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
  /** Workspace files to open and save (View → Open Workspace…). */
  workspaces?: WorkspaceApi;
  /** Screens to show and pick from (View → Screen…); without it the
   * window is the default screen. */
  screens?: ScreenApi;
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

export class Workspace {
  readonly root: HTMLDivElement;
  readonly sidebar: Sidebar;
  private readonly shortcuts: ShortcutSheet;
  /** Settings, when the app gives it what it needs. */
  readonly settings: SettingsModal | null;
  private readonly picker: ThemePicker | null;
  private readonly workspacePicker: WorkspacePicker | null;
  private readonly screenPicker: ScreenPicker | null;
  /** Where the screen goes. */
  private readonly frame: HTMLDivElement;
  /** Keeps the terminals while a screen doesn't show them. */
  private readonly holder: HTMLDivElement;
  private screenView: ScreenView | null = null;
  private parts: PartHost;
  /** The screen kept (not one being tried in the picker). */
  private screenNow: ScreenNow = DEFAULT_SCREEN;
  /** The screen's variables set on the root (`chrome.colors`). */
  private chromeVars: string[] = [];
  /** The terminals block shows only the focused pane. */
  solo = false;
  private theme: Theme = KIBANN_DARK;
  /** Part errors already shown, so a part failing every second says it once. */
  private readonly partErrors = new Set<string>();
  private readonly notices = new Notices();
  readonly tabBar: TabBar;
  readonly stage: HTMLDivElement;
  readonly tabs: Tab[] = [];
  readonly panes = new Map<PaneId, Pane>();
  active: Tab | null = null;
  private readonly platform: Platform;
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
    // The screen decides where the sidebar, the tab bar and the panes go.
    this.frame = document.createElement("div");
    this.frame.className = "workspace-screen";
    this.holder = document.createElement("div");
    this.holder.className = "workspace-holder";
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
      onUpdate: opts.settings
        ? () => void opts.settings?.installUpdate().catch((e) => this.notice(`Couldn't update: ${e}`))
        : undefined,
      settingsKey: () => keyFor({ kind: "settings" }, this.platform),
      shortcutKey: sheetKey,
      sidebarKey: () => keyFor({ kind: "sidebar" }, this.platform),
    });
    this.tabBar = new TabBar({ onHelp: () => this.shortcuts.toggle(), helpKey: sheetKey });
    this.stage = document.createElement("div");
    this.stage.className = "stage";

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
    this.root.append(this.frame, this.holder, this.notices.el, this.shortcuts.el);
    if (this.settings) this.root.append(this.settings.el);
    if (this.picker) this.root.append(this.picker.el);
    this.workspacePicker = opts.workspaces
      ? new WorkspacePicker(opts.workspaces, {
          onClose: () => this.focusedPane?.view?.focus(),
          notice: (text) => this.notice(text),
        })
      : null;
    if (this.workspacePicker) this.root.append(this.workspacePicker.el);
    this.screenPicker = opts.screens
      ? new ScreenPicker(opts.screens, {
          current: () => this.screenView?.name ?? this.screenNow.name,
          preview: (now) => this.showScreen(now ?? this.screenNow),
          onClose: () => this.focusedPane?.view?.focus(),
          notice: (text) => this.notice(text),
        })
      : null;
    if (this.screenPicker) this.root.append(this.screenPicker.el);
    this.parts = this.makeParts();
    this.showScreen(this.screenNow);
    if (opts.firstRunHint) this.notices.hintOnce(sheetKey());
    container.append(this.root);

    // Pulsing dots don't animate in a hidden window.
    const onVisible = () => this.root.classList.toggle("kibann-hidden", document.hidden);
    document.addEventListener("visibilitychange", onVisible);
    this.cleanups.push(() => document.removeEventListener("visibilitychange", onVisible));

    // What the user looks at changes with the window's focus too. After
    // the event: focus moving into a part's frame blurs the window first.
    const onWindowFocus = () => setTimeout(() => this.reportLooking(), 0);
    window.addEventListener("focus", onWindowFocus);
    window.addEventListener("blur", onWindowFocus);
    this.cleanups.push(
      () => window.removeEventListener("focus", onWindowFocus),
      () => window.removeEventListener("blur", onWindowFocus),
    );

    const ro = new ResizeObserver(() => this.active && this.layout(this.active));
    ro.observe(this.stage);
    const onKey = (e: KeyboardEvent) => this.onKeyDown(e);
    window.addEventListener("keydown", onKey, { capture: true });
    this.cleanups.push(
      () => ro.disconnect(),
      () => window.removeEventListener("keydown", onKey, { capture: true }),
    );
  }

  /** Whether the user is looking at the window. */
  windowFocused(): boolean {
    return this.opts.windowFocused?.() ?? document.hasFocus();
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
    if (action.kind === "workspaces" || action.kind === "saveWorkspace") {
      this.shortcuts.hide();
      this.settings?.hide();
      void this.workspacePicker?.open(action.kind === "workspaces" ? "open" : "save", on?.id ?? null);
      return;
    }
    if (action.kind === "screens") {
      this.shortcuts.hide();
      this.settings?.hide();
      void this.screenPicker?.open();
      return;
    }
    if (action.kind === "screenCycle") {
      void this.screenPicker?.cycle(action.step);
      return;
    }
    if (action.kind === "screenReset") {
      this.notice("Back to the default screen (View → Screen… to pick another).");
      if (this.opts.screens) void this.opts.screens.choose("default").catch((e) => this.notice(String(e)));
      else this.showScreen(DEFAULT_SCREEN);
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

  /** Where updating stands, for Settings → Updates and the sidebar's
   * update button. */
  setUpdateStatus(u: UpdateStatus): void {
    this.settings?.setUpdate(u);
    this.sidebar.setUpdate(u.state === "ready" ? u.version : null);
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
              canvas: () => p.view?.picture() ?? null,
              bg,
              cursor: () => (p.view ? cursorAt(p.view.grid) : 1),
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

  /**
   * A menu item fired (macOS; the other platforms have no menu bar). A
   * shortcut never arrives as both its key and its menu item: the page
   * prevents every key it maps, and WebKit hands the menu only keys the
   * page leaves alone.
   */
  runFromMenu(action: Action): void {
    this.run(action);
  }

  private onKeyDown(e: KeyboardEvent): void {
    const action = actionFor(e, this.platform);
    // While a picker or Settings is open a shortcut does nothing, and it's
    // prevented: on macOS WebKit hands a key the page leaves alone to the
    // menu, which would run it behind them (a split with Settings open).
    // A picker's search field still gets every key (prevented, not stopped).
    if (this.picker?.visible || this.workspacePicker?.visible || this.screenPicker?.visible) {
      if (action) e.preventDefault();
      return;
    }
    // Settings takes the keys while open (its fields): Esc closes it, and
    // so does its own shortcut. Nothing reaches the panes.
    if (this.settings?.visible) {
      if (e.key === "Escape" || action?.kind === "settings") {
        e.preventDefault();
        e.stopPropagation();
        if (e.key === "Escape") this.settings.hide();
        else this.run(action!);
      } else if (action) e.preventDefault();
      return;
    }
    // While the shortcut sheet is open, a key closes it and goes nowhere
    // else (modifiers alone don't, so a shortcut can be pressed to try it).
    if (this.shortcuts.visible && !["Shift", "Control", "Alt", "Meta"].includes(e.key)) {
      e.preventDefault();
      e.stopPropagation();
      // Its own shortcut closes it by toggling.
      if (action?.kind !== "shortcuts") this.shortcuts.hide();
      if (action) this.run(action);
      return;
    }
    if (!action) return;
    e.preventDefault();
    e.stopPropagation();
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
      tab.title = lt.title ?? null;
      tab.focused = lt.focused;
      tab.zoomed = lt.zoomed;
      return tab;
    });
    for (const t of this.tabs) {
      if (next.includes(t)) continue;
      t.el.remove();
      t.button.el.remove();
    }
    this.tabs.splice(0, this.tabs.length, ...next);

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
    this.reportLooking();
    if (hadTabs && next.length === 0) this.opts.onEmpty?.();
  }

  /** What the backend was last told the user looks at. */
  private lookingSent: PaneId | null | undefined = undefined;

  /** Tell the backend which pane the user looks at, when that changed:
   * the focused one of the shown tab, while the window has focus. */
  private reportLooking(): void {
    const focused = this.windowFocused();
    const tab = this.active;
    const pane = focused && tab ? (tab.zoomed ?? tab.focused) : null;
    if (pane === this.lookingSent) return;
    this.lookingSent = pane;
    this.opts.backend.looking?.(pane);
  }

  /** Show the daemon's agent status: pane dots, tab dots, the badge. */
  setStatus(map: StatusMap): void {
    const prev = this.status;
    this.status = map;
    if (this.statusSeen && this.notifyOn && this.opts.onNotify) {
      const focused = this.windowFocused();
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
    el.style.display = "none";
    this.stage.append(el);
    // The handlers run later, once `tab` is made.
    const button = this.tabBar.button(makeDot(), {
      activate: () => this.activate(tab),
      close: () => {
        for (const id of panes(tab.tree)) this.send({ t: "close", pane: id });
      },
    });
    const tab: Tab = { id, title: null, tree, focused, zoomed: null, el, button };
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

  private updateBar(): void {
    this.renderSidebar();
    this.parts.changed();
    this.tabBar.render(
      this.tabs.map((t) => {
        const branch = branchOf(this.git[String(t.focused)]);
        return { button: t.button, title: this.tabName(t) + (branch ? ` · ${branch}` : ""), active: t === this.active };
      }),
    );
    for (const t of this.tabs) showDot(t.button.dot, rollUp(this.status, panes(t.tree)));
  }

  /** Color the window around the terminals for `theme`. */
  setTheme(theme: Theme): void {
    this.theme = theme;
    applyChrome(theme);
    this.parts.themeChanged();
  }

  /** Show `text` in the corner for a while (see `Notices.show`). */
  notice(text: string, ms = 8000, action?: { label: string; run(): void }): void {
    this.notices.show(text, ms, action);
  }

  private emitTitle(): void {
    const p = this.focusedPane;
    this.opts.onTitle?.(p?.title || "Kibann Terminal");
  }

  // ---- screens ----

  /** Show `now` and keep it: the config's screen (or a fix to it). */
  useScreen(now: ScreenNow): void {
    this.screenNow = now;
    if (!this.screenPicker?.visible) this.showScreen(now);
    if (now.problem) this.notice(`Screen: ${now.problem}`, 12000);
  }

  /** The screen showing now. */
  get screenName(): string {
    return this.screenView?.name ?? this.screenNow.name;
  }

  /** Put `now` in the window (kept or being tried). */
  private showScreen(now: ScreenNow): void {
    this.parts.dispose();
    this.parts = this.makeParts();
    this.screenView?.dispose();
    for (const el of [this.sidebar.el, this.tabBar.el, this.stage]) el.remove();
    for (const k of this.chromeVars) this.root.style.removeProperty(k);
    this.chromeVars = [];
    const chrome = now.screen.chrome;
    for (const [k, v] of Object.entries(chrome.colors)) {
      this.root.style.setProperty(`--k-${k}`, v);
      this.chromeVars.push(`--k-${k}`);
    }
    if (chrome.font) {
      this.root.style.setProperty("--k-font", chrome.font);
      this.chromeVars.push("--k-font");
    }
    let terminals = false;
    this.solo = false;
    this.screenView = new ScreenView(now.name, now.screen, {
      fill: (slot) => {
        const c = slot.content;
        if (c.tab !== undefined || c.pane !== undefined) {
          terminals = true;
          this.solo = c.pane !== undefined;
          slot.el.append(this.stage);
        } else if (c.part === "sidebar") {
          this.sidebar.el.style.flex = "0 0 auto";
          slot.el.append(this.sidebar.el);
        } else if (c.part === "tabs") {
          slot.el.append(this.tabBar.el);
        } else if (c.part) {
          const part = now.parts[c.part];
          if (part) this.parts.mount(c.part, part, c.props, slot);
          else this.slotProblem(slot, `no part ${c.part}`);
        }
      },
      onShown: (slot) => this.parts.shown(slot),
    });
    this.root.dataset.screen = now.name;
    this.frame.replaceChildren(this.screenView.el);
    if (!terminals) this.holder.append(this.stage);
    if (this.active) this.layout(this.active);
    this.updateBar();
  }

  private slotProblem(slot: BlockSlot, text: string): void {
    const d = document.createElement("div");
    d.className = "slot-problem";
    d.textContent = text;
    slot.el.append(d);
  }

  private makeParts(): PartHost {
    return new PartHost({
      state: () => partState(this),
      theme: () => partTheme(this.theme, this.screenView?.screen.chrome),
      run: (part, action, args) => runPartAction(this.partActions(), part, action, args),
      exec: (part, program, args, cwd, timeout) => {
        if (!this.opts.screens) return Promise.reject(new Error("can't run programs here"));
        return this.opts.screens.exec(part, program, args, cwd, timeout);
      },
      thumbnail: async (pane, width, height) => {
        const view = this.panes.get(pane)?.view;
        return view ? paneThumbnail(view, width, height) : null;
      },
      key: (k, granted) => {
        const action = actionFor({ ...k, repeat: false, isComposing: false, keyCode: 0 }, this.platform);
        if (!action || (action.kind === "close" && !granted.type)) return;
        this.run(action);
      },
      release: () => this.focusedPane?.view?.focus(),
      error: (part, message) => {
        const key = `${part}:${message}`;
        if (this.partErrors.has(key)) return;
        this.partErrors.add(key);
        console.warn(`kibann: part ${part}: ${message}`);
        this.notice(`Part ${part}: ${message}`);
      },
    });
  }

  /** What parts' actions may do here. */
  private partActions(): PartActions {
    return {
      pane: (id) => this.panes.get(id),
      tab: (id) => this.tabs.find((t) => t.id === id),
      focused: () => this.focusedPane,
      show: (id) => this.show(id),
      activate: (t) => this.activate(t),
      send: (a) => this.send(a),
      run: (action, p) => this.run(action, p),
      diff: (p) => this.opts.onDiff?.(p),
      workspaces: this.opts.workspaces,
      screens: this.opts.screens,
      setNode: (id, how) => {
        if (!this.screenView?.set(id, how)) return false;
        if (this.active) this.layout(this.active);
        return true;
      },
      notice: (text) => this.notice(text),
    };
  }

  /** What a tab is called: the name it was given, or its focused pane's. */
  private tabName(t: Tab): string {
    return tabName(t, this.panes);
  }

  // ---- panes ----

  private addPane(tab: Tab, id: PaneId, transport: Transport): Pane {
    const el = document.createElement("div");
    el.className = "pane";
    el.dataset.pane = String(id);
    const dot = makeDot();
    const header = new PaneHeader(dot, () => {
      const p = this.panes.get(id);
      if (p) this.opts.onDiff?.(p);
    });
    header.show(this.git[String(id)]);
    header.setActive(tab.focused === id);
    // Double-clicking the header zooms the split, and again restores the
    // layout (as Cmd+Shift+Enter). Not on ●N, which opens the diff.
    header.el.addEventListener("dblclick", (e) => {
      if ((e.target as HTMLElement).classList.contains("pane-changed")) return;
      e.preventDefault();
      const p = this.panes.get(id);
      if (p) this.run({ kind: "zoom" }, p);
    });
    // The terminal fills what the header leaves.
    const body = document.createElement("div");
    body.className = "pane-body";
    body.style.top = `${HEADER_H}px`;
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
      this.showPanes();
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
    if (this.solo && tab === this.active) this.layout(tab);
    if (tab === this.active) {
      this.updateBar();
      this.emitTitle();
      this.reportLooking();
    }
  }

  // ---- layout ----

  private box(): Rect {
    return { x: 0, y: 0, w: this.stage.clientWidth, h: this.stage.clientHeight };
  }

  /** Position a tab's panes and dividers in the stage. */
  layout(tab: Tab): void {
    this.place(tab);
    if (tab.zoomed !== null || this.solo) {
      for (const d of tab.el.querySelectorAll(".divider")) d.remove();
      return;
    }
    makeDividers(
      tab.el,
      () => tab.tree,
      () => this.box(),
      () => this.stage.getBoundingClientRect(),
      {
        drag: (path, ratio) => {
          tab.tree = setRatio(tab.tree, path, ratio);
          this.place(tab);
        },
        drop: (path, ratio) => {
          this.layout(tab);
          this.focusPane(tab.focused);
          if (ratio !== null) this.send({ t: "set_ratio", tab: tab.id, path, ratio });
        },
      },
    );
  }

  /** Tell each pane whether it's on screen: hidden ones don't paint. */
  private showPanes(): void {
    for (const p of this.panes.values()) {
      p.view?.setShown(p.tab === this.active && p.el.style.display !== "none");
    }
  }

  /** Position the panes only. */
  private place(tab: Tab): void {
    const els = panes(tab.tree).flatMap((id): Array<[PaneId, HTMLElement]> => {
      const p = this.panes.get(id);
      return p ? [[id, p.el]] : [];
    });
    place(els, paneRects(tab, this.box(), this.solo));
    this.showPanes();
    // Nothing observes sizes in a hidden page: measure here instead.
    if (document.hidden) for (const [id] of els) this.panes.get(id)?.view?.fit();
  }

  dispose(): void {
    for (const p of [...this.panes.values()]) {
      p.closed = true;
      p.view?.dispose();
      p.transport.close?.();
    }
    for (const c of this.cleanups) c();
    this.parts.dispose();
    this.screenView?.dispose();
    this.sidebar.dispose();
    this.root.remove();
  }
}
