// Tabs and splits in a browser, with fake transports and a fake backend in
// place of the daemon: each pane shows its number and logs its input.
// Playwright drives it through `window.__ws`.
//
// Params: w, h (CSS px), platform=mac|other, dpr, saved (a layout to start
// from, as the daemon would have it), links=1 (each pane shows a link the
// fake backend reports on hover).

import "@fontsource/jetbrains-mono/400.css";
import type { InputEvent } from "../proto";
import { ReplayTransport } from "../transport/replay";
import { FakeBackend } from "../workspace/backend";
import { Workspace } from "../workspace/workspace";
import { KIBANN_DARK, type ThemeApi } from "../workspace/theme";
import type { Settings, Theme } from "../proto";
import { Screen } from "./screen";

const params = Object.fromEntries(new URLSearchParams(location.search));
const root = document.getElementById("ws")!;
if (params.w) root.style.setProperty("--w", `${params.w}px`);
if (params.h) root.style.setProperty("--h", `${params.h}px`);

const backend = new FakeBackend(params.saved ? JSON.parse(params.saved) : undefined);
const transports: ReplayTransport[] = [];
/** Input per transport number, in order. */
const inputs: Array<{ n: number; e: InputEvent }> = [];
let titles: string[] = [];
let quit = false;
/** Targets opened with Cmd+click. */
const opened: string[] = [];

function transport(n: number): ReplayTransport {
  const from = backend.origin.get(n) ?? 0;
  const t = new ReplayTransport({
    build: (cols, rows) => {
      const s = new Screen(cols, rows);
      s.write(0, 0, `pane ${n}${from ? ` from ${from}` : ""}`);
      s.write(0, 1, `${cols}x${rows}`);
      if (params.links) {
        // A link wrapped over two rows, as the backend would report it.
        s.write(0, 2, "see https://example.com/very/");
        s.write(0, 3, "long/path ok");
      }
      return s;
    },
  });
  if (params.links) t.links = [
    {
      uri: "https://example.com/very/long/path",
      target: "https://example.com/very/long/path",
      spans: [
        { y: 2, x: 4, len: 25 },
        { y: 3, x: 0, len: 9 },
      ],
    },
  ];
  t.onInput = (e) => inputs.push({ n, e });
  t.onExit = () => backend.exited(n);
  transports.push(t);
  return t;
}

// A few themes, as the app would list them; `previews` records each step.
const THEMES: Record<string, Theme> = {
  "Kibann Dark": KIBANN_DARK,
  "Kibann Light": {
    ...KIBANN_DARK,
    palette: [...KIBANN_DARK.palette.slice(0, 4), 0x0969da, ...KIBANN_DARK.palette.slice(5)],
    background: 0xffffff,
    foreground: 0x1f2328,
  },
  Dracula: { ...KIBANN_DARK, palette: [...KIBANN_DARK.palette.slice(0, 4), 0xbd93f9, ...KIBANN_DARK.palette.slice(5)], background: 0x282a36, foreground: 0xf8f8f2 },
  Nord: { ...KIBANN_DARK, background: 0x2e3440, foreground: 0xd8dee9 },
  "Solarized Light": { ...KIBANN_DARK, background: 0xfdf6e3, foreground: 0x657b83 },
};
let kept = "Kibann Dark";
let showing = kept;
const previews: Array<string | null> = [];
const themes: ThemeApi = {
  list: async () =>
    Object.entries(THEMES).map(([name, t]) => ({ name, dark: name !== "Kibann Light" && name !== "Solarized Light" })),
  preview: async (name) => {
    previews.push(name);
    showing = name ?? kept;
    ws.setTheme(THEMES[showing]);
  },
  choose: async (name) => {
    kept = showing = name;
    ws.setTheme(THEMES[name]);
  },
  current: () => kept,
};

const ws = new Workspace(root, {
  themes,
  firstRunHint: params.hint === "1",
  backend,
  transport,
  platform: params.platform === "mac" || params.platform === "other" ? params.platform : undefined,
  view: { dpr: params.dpr ? Number(params.dpr) : undefined, onOpenLink: (t) => opened.push(t) },
  onTitle: (t) => titles.push(t),
  onEmpty: () => (quit = true),
  onBlocked: (n) => (badge = n),
  onDiff: (p) => diffs.push(p.id),
  onNotify: (n) => notes.push(n),
  windowFocused: () => windowFocused,
});
/** System notifications asked for, and whether the window "has focus". */
const notes: Array<{ pane: number; title: string; body: string }> = [];
let windowFocused = true;
backend.onLayout = (l) => ws.apply(l);
backend.onStatus = (m) => ws.setStatus(m);
backend.onGit = (m) => ws.setGit(m);
// The daemon sends its status map right after the first layout.
ws.setStatus({});
let badge = 0;
/** Panes whose diff was asked for. */
const diffs: number[] = [];
// `saved`: a layout from `__ws.snapshot()`, as if the daemon kept it.
if (params.saved) ws.apply(backend.layout);
else void backend.act({ t: "new_tab", focus: true });

(window as any).__ws = {
  ws,
  transports,
  inputs,
  opened,
  get titles() {
    return titles;
  },
  get quit() {
    return quit;
  },
  get badge() {
    return badge;
  },
  diffs,
  previews,
  notes,
  /** The window gains or loses focus (as far as notifications care). */
  setWindowFocused(on: boolean) {
    windowFocused = on;
  },
  /** The config file's settings changed. */
  setSettings(s: Partial<Settings>) {
    ws.setSettings({ font_family: [], font_size: null, agent_notifications: true, keybind: [], restore_layout: true, ...s });
  },
  /** The font size in use, and each pane's cell size (device px). */
  font() {
    return {
      size: ws.fontSize,
      cells: [...ws.panes.values()].map((p) => (p.view ? [p.view.m.cellW, p.view.m.cellH] : null)),
    };
  },
  /** The theme kept and the one showing. */
  theme: () => ({ kept, showing }),
  /** Show a theme as the app would after a config change. */
  setTheme(name: string) {
    kept = showing = name;
    ws.setTheme(THEMES[name]);
  },
  /** The shortcut sheet: shown or not, and each row as "label: keys". */
  shortcuts() {
    const el = document.querySelector<HTMLElement>(".shortcut-sheet")!;
    return {
      visible: el.style.display !== "none",
      rows: [...el.querySelectorAll(".shortcut-row")].map(
        (r) => `${r.querySelector("span")!.textContent}: ${[...r.querySelectorAll("kbd")].map((k) => k.textContent).join(" | ")}`,
      ),
    };
  },
  /** The sidebar's groups and cards, in order. */
  sidebar() {
    const els = [...ws.sidebar.el.querySelectorAll<HTMLElement>(".sidebar-group, .sidebar-card")];
    return {
      expanded: ws.sidebar.expanded,
      width: ws.sidebar.width,
      items: els.map((e) =>
        e.classList.contains("sidebar-group") ? `# ${e.textContent}` : Number(e.dataset.pane),
      ),
    };
  },
  /** A pane's agent changes state (as the daemon would detect it). */
  setAgent(pane: number, state: string | null) {
    backend.setAgent(pane, state as never);
  },
  /** Where a pane is, as the daemon would report it. */
  setPlace(pane: number, place: unknown) {
    backend.setPlace(pane, place as never);
  },
  /** Each pane's header text, and each tab's title. */
  headers() {
    return {
      panes: Object.fromEntries(
        [...ws.panes.values()].map((p) => [p.id, [...p.header.el.querySelectorAll(".pane-place > span")].map((s) => s.textContent).join(" ")]),
      ),
      tabs: ws.tabs.map((t) => t.button.title),
    };
  },
  /** Which dots show: per pane, and per tab in bar order. */
  dots() {
    const pick = (el: Element | null) => (el as HTMLElement | null)?.dataset.state ?? null;
    return {
      panes: Object.fromEntries([...ws.panes.values()].map((p) => [p.id, pick(p.dot)])),
      tabs: ws.tabs.map((t) => pick(t.dot)),
    };
  },
  /** Pane ids of the active tab in reading order, with the focused one. */
  state() {
    const tab = ws.active!;
    const els = [...tab.el.querySelectorAll<HTMLElement>(".pane")].filter((e) => e.style.display !== "none");
    return {
      tabs: ws.tabs.length,
      active: ws.tabs.indexOf(tab),
      focused: tab.focused,
      zoomed: tab.zoomed,
      panes: els.map((e) => {
        const r = e.getBoundingClientRect();
        return { id: Number(e.dataset.pane), x: r.x, y: r.y, w: r.width, h: r.height };
      }),
    };
  },
  /** The layout as the backend keeps it. */
  snapshot() {
    return backend.layout;
  },
  /** An action from an agent (the CLI), not the app. */
  agent(action: unknown) {
    return backend.external(action as never);
  },
  /** Every pane's view is created. */
  ready() {
    return [...ws.panes.values()].every((p) => p.view !== null);
  },
};
