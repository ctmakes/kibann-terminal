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

const ws = new Workspace(root, {
  backend,
  transport,
  platform: params.platform === "mac" || params.platform === "other" ? params.platform : undefined,
  view: { dpr: params.dpr ? Number(params.dpr) : undefined, onOpenLink: (t) => opened.push(t) },
  onTitle: (t) => titles.push(t),
  onEmpty: () => (quit = true),
});
backend.onLayout = (l) => ws.apply(l);
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
