// Tabs and splits in a browser, with fake transports: each pane shows its
// number and logs its input. Playwright drives it through `window.__ws`.
//
// Params: w, h (CSS px), platform=mac|other, dpr.

import "@fontsource/jetbrains-mono/400.css";
import type { InputEvent } from "../proto";
import { ReplayTransport } from "../transport/replay";
import type { Transport } from "../transport/types";
import { Workspace } from "../workspace/workspace";
import { Screen } from "./screen";

const params = Object.fromEntries(new URLSearchParams(location.search));
const root = document.getElementById("ws")!;
if (params.w) root.style.setProperty("--w", `${params.w}px`);
if (params.h) root.style.setProperty("--h", `${params.h}px`);

let made = 0;
const transports: ReplayTransport[] = [];
/** Input per transport number, in order. */
const inputs: Array<{ n: number; e: InputEvent }> = [];
let titles: string[] = [];
let quit = false;

function transport(near: Transport | null): Transport {
  const n = ++made;
  const from = near ? transports.indexOf(near as ReplayTransport) + 1 : 0;
  const t = new ReplayTransport({
    build: (cols, rows) => {
      const s = new Screen(cols, rows);
      s.write(0, 0, `pane ${n}${from ? ` from ${from}` : ""}`);
      s.write(0, 1, `${cols}x${rows}`);
      return s;
    },
  });
  t.onInput = (e) => inputs.push({ n, e });
  transports.push(t);
  return t;
}

const ws = new Workspace(root, {
  transport,
  platform: params.platform === "mac" || params.platform === "other" ? params.platform : undefined,
  view: { dpr: params.dpr ? Number(params.dpr) : undefined },
  onTitle: (t) => titles.push(t),
  onEmpty: () => (quit = true),
});
ws.newTab(transport(null));

(window as any).__ws = {
  ws,
  transports,
  inputs,
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
  /** Every pane's view is created. */
  ready() {
    return [...ws.panes.values()].every((p) => p.view !== null);
  },
};
