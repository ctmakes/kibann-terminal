// The painter's saved answers, shared with the phone app's painter (see
// fixtures/paint/README.md): input frames, and the display list this
// painter makes for each. A change here that alters what's drawn fails
// until the answers are rewritten with `npm run gen:paint`, so every
// visible change shows in the diff of fixtures/paint/expected.

import { mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { Flag, type Frame } from "../proto";
import { Grid } from "../grid";
import { computeMetrics, type Metrics } from "../metrics";
import { buildFrame, Screen, style } from "../dev/screen";
import { cursorColumn, planCursor, planRow } from "./plan";

const ROOT = fileURLToPath(new URL("../../../fixtures/paint/", import.meta.url));
const RECORDINGS = fileURLToPath(new URL("../dev/recordings/", import.meta.url));
const UPDATE = !!process.env.UPDATE_PAINT;

/** JetBrains Mono's proportions: advance 0.6em, ascent 1.02em, descent 0.3em. */
function metricsAt(fontSize: number, dpr: number): Metrics {
  return computeMetrics({ advance: 0.6 * fontSize, ascent: 1.02 * fontSize, descent: 0.3 * fontSize }, fontSize, dpr);
}

const METRICS: Record<string, Metrics> = {
  "1x": metricsAt(14, 1),
  "2x": metricsAt(14, 2),
  // Fractional scale and an odd font size: cells and lines that don't
  // divide evenly, where geometry rounding goes wrong first.
  odd: metricsAt(13, 1.5),
};

// ---- synthetic inputs ----

const FG = 0xd8d8d8;

/** One full frame, then cursor-only changes: every shape, on wide heads
 * and tails, on box characters, at the edge, hidden and out of view. */
function cursorScreen(): Frame[] {
  const s = new Screen(10, 3);
  s.write(0, 0, "ab日c═d", style(FG, 0x202040));
  s.write(0, 1, "hid", style(FG, 0x333333, Flag.INVISIBLE));
  s.write(8, 2, "日");
  const moves: Array<Partial<Screen["cursor"]>> = [
    { x: 0, y: 0, style: "block" },
    { x: 0, y: 0, style: "block_hollow" },
    { x: 0, y: 0, style: "bar" },
    { x: 0, y: 0, style: "underline" },
    { x: 2, y: 0, style: "block" }, // a wide head
    { x: 3, y: 0, style: "block" }, // its tail: drawn on the head
    { x: 5, y: 0, style: "block" }, // a box character
    { x: 0, y: 1, style: "block" }, // invisible text
    { x: 8, y: 2, style: "underline" }, // a wide character in the last two columns
    { x: 9, y: 1, style: "underline", wide: true }, // told it's wide, in the last column: one cell
    { x: 1, y: 0, style: "block", color: 0x23d18b },
    { x: 1, y: 0, style: "block", visible: false },
    { x: 1, y: 0, style: "block", in_viewport: false },
  ];
  const frames = [buildFrame(1, s, null)];
  let prev = s;
  moves.forEach((mv, i) => {
    const next = prev.clone();
    next.cursor = { ...next.cursor, visible: true, in_viewport: true, wide: false, color: null, ...mv };
    frames.push(buildFrame(i + 2, next, prev));
    prev = next;
  });
  return frames;
}

/** An input the emulator drew from escape sequences, as saved by
 * `crates/term/tests/paint.rs`: the cells, widths and styles a program's
 * output really gives. */
function emulated(name: string): Frame[] {
  return readFileSync(`${ROOT}inputs/${name}.jsonl`, "utf8")
    .split("\n")
    .filter((l) => l.trim())
    .map((l) => JSON.parse(l) as Frame);
}

/** A recording from `dev/recordings`, the frames only. */
function recording(name: string): Frame[] {
  return readFileSync(`${RECORDINGS}${name}.jsonl`, "utf8")
    .split("\n")
    .filter((l) => l.trim())
    .map((l) => (JSON.parse(l) as { frame: Frame }).frame);
}

/** Each input and the metric sets it's answered at. The recordings are
 * real programs with partial frames (the frame → grid step); the metric
 * sets matter for geometry, which the synthetic screens cover. Every input
 * is saved with the answers here, except the emulator's (`emulator`),
 * which it saves itself. The cursor screen is made here: it has cursors no
 * program can ask for (wide in the last column). */
const INPUTS: Array<{ name: string; frames: () => Frame[]; metrics: string[]; emulator?: boolean }> = [
  { name: "box", frames: () => emulated("box"), metrics: ["1x", "2x", "odd"], emulator: true },
  { name: "styles", frames: () => emulated("styles"), metrics: ["1x", "2x", "odd"], emulator: true },
  { name: "wide", frames: () => emulated("wide"), metrics: ["1x", "2x", "odd"], emulator: true },
  { name: "cursor", frames: cursorScreen, metrics: ["1x", "2x", "odd"] },
  { name: "man", frames: () => recording("man"), metrics: ["1x"] },
  { name: "sync", frames: () => recording("sync"), metrics: ["1x"] },
  { name: "vim", frames: () => recording("vim"), metrics: ["1x"] },
];

// ---- answers ----

/**
 * What the painter draws after each frame, as the terminal view does:
 * every row after a full or resizing frame, else the changed rows; then
 * the cursor (focused, blink on). Per frame: a line saying whether it
 * applied, a line per row painted, and a line for the cursor, so a diff
 * points at the row that changed.
 */
function answer(frames: Frame[], m: Metrics): string {
  const g = new Grid();
  const lines: string[] = [];
  for (const f of frames) {
    const r = g.apply(f);
    if (!r.applied) {
      lines.push(JSON.stringify({ seq: f.seq, applied: false, requestFull: r.requestFull }));
      continue;
    }
    lines.push(JSON.stringify({ seq: f.seq, applied: true, cols: g.cols, rows: g.rows }));
    const ys = r.full || r.resized ? [...Array(g.rows).keys()] : [...r.dirty].sort((a, b) => a - b);
    for (const y of ys) if (y < g.rows) lines.push(JSON.stringify({ seq: f.seq, row: y, ops: planRow(g, y, g.cols, m) }));
    const c = g.cursor;
    const cursor =
      c.in_viewport && c.visible && c.y < g.rows ? planCursor(g, cursorColumn(g), c.y, g.cols, c.style, m) : null;
    lines.push(JSON.stringify({ seq: f.seq, cursor }));
  }
  return lines.join("\n") + "\n";
}

/** Every fixture file, by path under fixtures/paint. */
function build(): Map<string, string> {
  const files = new Map<string, string>();
  files.set("metrics.json", JSON.stringify(METRICS, null, 2) + "\n");
  for (const input of INPUTS) {
    const frames = input.frames();
    if (!input.emulator) files.set(`inputs/${input.name}.jsonl`, frames.map((f) => JSON.stringify(f)).join("\n") + "\n");
    for (const name of input.metrics) {
      files.set(`expected/${input.name}.${name}.jsonl`, answer(frames, METRICS[name]));
    }
  }
  return files;
}

function listed(dir: string): string[] {
  try {
    return readdirSync(ROOT + dir).map((f) => `${dir}/${f}`);
  } catch {
    return [];
  }
}

/** Where `a` and `b` first differ: the line, and in a row or cursor line
 * the first op that differs. */
function firstDifference(a: string, b: string): string {
  const x = a.split("\n");
  const y = b.split("\n");
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    if (x[i] === y[i]) continue;
    const where = `line ${i + 1}`;
    try {
      const s = JSON.parse(x[i]) as { seq?: number; row?: number; ops?: unknown[]; cursor?: unknown[] | null };
      const n = JSON.parse(y[i]) as typeof s;
      const so = s.ops ?? s.cursor;
      const no = n.ops ?? n.cursor;
      if (Array.isArray(so) && Array.isArray(no) && s.seq === n.seq && s.row === n.row) {
        const what = s.row === undefined ? "cursor" : `row ${s.row}`;
        for (let j = 0; j < Math.max(so.length, no.length); j++) {
          const sj = JSON.stringify(so[j]);
          const nj = JSON.stringify(no[j]);
          if (sj !== nj) return `${where} (seq ${s.seq}, ${what}), op ${j}\n  saved: ${sj}\n  now:   ${nj}`;
        }
      }
    } catch {
      // Not JSON: show the lines.
    }
    const cut = (s = "") => (s.length > 300 ? `${s.slice(0, 300)}…` : s);
    return `${where}\n  saved: ${cut(x[i])}\n  now:   ${cut(y[i])}`;
  }
  return "same";
}

describe("paint fixtures (fixtures/paint)", () => {
  const files = build();

  if (UPDATE) {
    it("rewrites them", () => {
      rmSync(ROOT + "expected", { recursive: true, force: true });
      for (const i of INPUTS) if (!i.emulator) rmSync(`${ROOT}inputs/${i.name}.jsonl`, { force: true });
      for (const [path, text] of files) {
        mkdirSync(ROOT + path.slice(0, path.lastIndexOf("/") + 1), { recursive: true });
        writeFileSync(ROOT + path, text);
      }
    });
    return;
  }

  it("has no files the painter doesn't make", () => {
    const theirs = new Set(INPUTS.filter((i) => i.emulator).map((i) => `inputs/${i.name}.jsonl`));
    const stale = [...listed("inputs"), ...listed("expected")].filter((p) => !files.has(p) && !theirs.has(p));
    expect(stale, "stale fixture files: run `npm run gen:paint`").toEqual([]);
  });

  for (const [path, text] of files) {
    it(path, () => {
      let saved: string;
      try {
        saved = readFileSync(ROOT + path, "utf8");
      } catch {
        throw new Error(`fixtures/paint/${path} is missing: run \`npm run gen:paint\``);
      }
      if (saved !== text) {
        throw new Error(
          `fixtures/paint/${path} differs from what the painter draws now (run \`npm run gen:paint\` if the change is meant):\n` +
            firstDifference(saved, text),
        );
      }
    });
  }
});
