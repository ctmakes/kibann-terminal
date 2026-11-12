// Browser-only harness: renders a fixture through the real TerminalView with a
// ReplayTransport standing in for the backend.
//
// Query params: fixture, w, h (CSS px of the terminal), font, dpr, bare=1
// (no header or input log), platform=mac|other, plus fixture params (style, wide…).
// replay=NAME plays app/src/dev/recordings/NAME.jsonl instead of a fixture
// (frames from the real emulator; see crates/term/recordings), with at=end|MS
// to jump to a point and speed=N for real-time play.

import { TerminalView, type PaintInfo } from "../terminal";
import { RecordingTransport, parseFrames } from "../transport/recording";
import { ReplayTransport } from "../transport/replay";
import { FIXTURES, fixture as findFixture, type Params } from "./fixtures";

const q = new URLSearchParams(location.search);
const params: Params = Object.fromEntries(q.entries());
const fx = findFixture(params.fixture ?? "palette") ?? FIXTURES[0];

const termEl = document.getElementById("term")!;
const logEl = document.getElementById("log")!;
const statsEl = document.getElementById("stats")!;
const select = document.getElementById("fixture") as HTMLSelectElement;

if (params.bare === "1") document.body.classList.add("bare");
if (params.w || params.h) {
  document.body.classList.add("fixed");
  termEl.style.width = `${Number(params.w ?? 800)}px`;
  termEl.style.height = `${Number(params.h ?? 480)}px`;
}

const recordings = import.meta.glob<string>("./recordings/*.jsonl", { query: "?raw", import: "default" });
const recordingName = (path: string) => path.replace(/^.*\/|\.jsonl$/g, "");
const replayLoader = params.replay
  ? Object.entries(recordings).find(([path]) => recordingName(path) === params.replay)?.[1]
  : undefined;
if (params.replay && !replayLoader) throw new Error(`no recording ${params.replay}`);

for (const f of FIXTURES) select.add(new Option(f.name, f.name, false, !replayLoader && f === fx));
const group = document.createElement("optgroup");
group.label = "Recordings";
for (const path of Object.keys(recordings)) {
  const name = recordingName(path);
  group.append(new Option(name, `replay:${name}`, false, name === params.replay));
}
select.append(group);
select.addEventListener("change", () => {
  q.delete("fixture");
  q.delete("replay");
  if (select.value.startsWith("replay:")) q.set("replay", select.value.slice(7));
  else q.set("fixture", select.value);
  location.search = q.toString();
});
document.getElementById("desc")!.textContent = replayLoader ? `recording: ${params.replay}` : fx.description;

const transport = replayLoader
  ? new RecordingTransport(parseFrames(await replayLoader()), {
      until: params.at === undefined ? undefined : params.at === "end" ? Infinity : Number(params.at),
      speed: params.speed ? Number(params.speed) : undefined,
    })
  : new ReplayTransport({
  interval: params.still === "1" ? undefined : fx.interval,
  build: (cols, rows, tick) => {
    const screen = fx.build(cols, rows, tick, params);
    if (params.mouse) screen.mouseTracking = params.mouse as typeof screen.mouseTracking;
    return screen;
  },
});

transport.onInput = (e) => {
  const div = document.createElement("div");
  div.textContent = JSON.stringify(e);
  logEl.prepend(div);
  while (logEl.childElementCount > 200) logEl.lastElementChild!.remove();
};

let paints = 0;
let contextMenus = 0;
let paintMs = 0;
const paintLog: PaintInfo[] = [];
let resolveReady: () => void;
const ready = new Promise<void>((r) => (resolveReady = r));

const view = await TerminalView.create(termEl, transport, {
  fontSize: params.font ? Number(params.font) : undefined,
  dpr: params.dpr ? Number(params.dpr) : undefined,
  platform: params.platform === "mac" || params.platform === "other" ? params.platform : undefined,
  onContextMenu: () => contextMenus++,
  onPaint: (info) => {
    paints++;
    paintMs = info.ms;
    paintLog.push(info);
    if (paintLog.length > 500) paintLog.shift();
    statsEl.textContent = `${view.grid.cols}×${view.grid.rows}  seq ${info.seq}  ${info.rows} rows  ${info.ms.toFixed(2)} ms`;
    resolveReady();
  },
});

function median(xs: number[]): number {
  const s = [...xs].sort((a, b) => a - b);
  return s[Math.floor(s.length / 2)];
}

/** Hooks for Playwright. */
const harness = {
  ready: false,
  view,
  transport,
  /** Times the right-click menu would have opened. */
  get contextMenus() {
    return contextMenus;
  },
  get paints() {
    return paints;
  },
  get paintMs() {
    return paintMs;
  },
  get inputs() {
    return transport.inputs;
  },
  paintLog,
  metrics: () => ({ ...view.m, cols: view.grid.cols, rows: view.grid.rows }),
  /** RGBA of one device pixel of the grid canvas. */
  pixel(x: number, y: number): [number, number, number, number] {
    const d = view.painter.ctx.getImageData(x, y, 1, 1).data;
    return [d[0], d[1], d[2], d[3]];
  },
  /** RGB of a horizontal or vertical line of pixels, packed as 0xRRGGBB. */
  line(x0: number, y0: number, len: number, vertical: boolean): number[] {
    const d = view.painter.ctx.getImageData(x0, y0, vertical ? 1 : len, vertical ? len : 1).data;
    const out: number[] = [];
    for (let i = 0; i < d.length; i += 4) out.push((d[i] << 16) | (d[i + 1] << 8) | d[i + 2]);
    return out;
  },
  /** Median paint times: a full repaint, and a 3-row partial repaint. */
  bench(n = 40) {
    view.painter.paintAll(view.grid); // warm caches
    const flush = () => view.painter.ctx.getImageData(0, 0, 1, 1);
    const full: number[] = [];
    const part: number[] = [];
    for (let i = 0; i < n; i++) {
      const t0 = performance.now();
      view.painter.paintAll(view.grid);
      flush();
      full.push(performance.now() - t0);
    }
    for (let i = 0; i < n; i++) {
      const t0 = performance.now();
      view.painter.paintRows(view.grid, [1, 2, 3]);
      flush();
      part.push(performance.now() - t0);
    }
    return { cols: view.grid.cols, rows: view.grid.rows, fullMs: median(full), rowsMs: median(part) };
  },
};

(window as unknown as { __harness: typeof harness }).__harness = harness;
await ready;
harness.ready = true;
