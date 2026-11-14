// The painter's saved answers, shared with the phone app's painter (see
// fixtures/paint/README.md): input frames, and the display list this
// painter makes for each. A change here that alters what's drawn fails
// until the answers are rewritten with `npm run gen:paint`, so every
// visible change shows in the diff of fixtures/paint/expected.

import { mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { Flag, Underline, withUnderline, type Frame } from "../proto";
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
const PALETTE = [
  0x000000, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5,
  0x666666, 0xf14c4c, 0x23d18b, 0xf5f543, 0x3b8eea, 0xd670d6, 0x29b8db, 0xffffff,
];

/** Every box-drawing, block, braille and Powerline character, 64 a row;
 * odd rows on a background (double lines show it in their gaps). */
function boxScreen(): Frame[] {
  const cps: number[] = [];
  for (let cp = 0x2500; cp <= 0x259f; cp++) cps.push(cp);
  for (let cp = 0x2800; cp <= 0x28ff; cp++) cps.push(cp);
  for (let cp = 0xe0b0; cp <= 0xe0b3; cp++) cps.push(cp);
  const cols = 64;
  const s = new Screen(cols, Math.ceil(cps.length / cols));
  cps.forEach((cp, i) => {
    const y = Math.floor(i / cols);
    s.write(i % cols, y, String.fromCodePoint(cp), y % 2 ? style(0x0dbc79, 0x202040) : style(FG));
  });
  s.cursor = { ...s.cursor, visible: false };
  return [buildFrame(1, s, null)];
}

/** Colors, fonts, faint, invisible and every decoration. */
function stylesScreen(): Frame[] {
  const s = new Screen(48, 12);
  PALETTE.forEach((c, i) => {
    s.write(i * 3, 0, "Ab", style(c));
    s.write(i * 3, 1, "Ab", style(FG, c));
  });
  s.write(0, 2, "regular", style(FG));
  s.write(8, 2, "bold", style(FG, null, Flag.BOLD));
  s.write(13, 2, "italic", style(FG, null, Flag.ITALIC));
  s.write(20, 2, "both", style(FG, null, Flag.BOLD | Flag.ITALIC));
  s.write(25, 2, "faint", style(FG, null, Flag.FAINT));
  s.write(31, 2, "hidden", style(FG, 0x333333, Flag.INVISIBLE | Flag.STRIKE));
  const uls = [Underline.SINGLE, Underline.DOUBLE, Underline.CURLY, Underline.DOTTED, Underline.DASHED];
  uls.forEach((ul, i) => {
    s.write(0, 3 + i, `underline ${ul}`, style(FG, null, withUnderline(0, ul)));
    s.write(16, 3 + i, `colored ${ul}`, style(FG, null, withUnderline(0, ul), 0xf14c4c));
    s.write(32, 3 + i, `faint ${ul}`, style(FG, 0x202040, withUnderline(Flag.FAINT, ul), 0x3b8eea));
  });
  s.write(0, 8, "strike", style(FG, null, Flag.STRIKE));
  s.write(8, 8, "overline", style(FG, null, Flag.OVERLINE));
  s.write(18, 8, "all", style(0xe5e510, null, withUnderline(Flag.STRIKE | Flag.OVERLINE, Underline.DOUBLE), 0x23d18b));
  // A decorated run that starts mid-cell-pattern and ends at the edge.
  s.write(45, 9, "end", style(FG, null, withUnderline(0, Underline.CURLY)));
  s.write(0, 10, "mixed ", style(FG));
  s.write(6, 10, "runs", style(0xcd3131, 0x101010, Flag.BOLD));
  s.write(10, 10, " in a row", style(FG, null, withUnderline(0, Underline.DASHED)));
  s.cursor = { ...s.cursor, visible: false };
  return [buildFrame(1, s, null)];
}

/** Wide and composed graphemes, including one that doesn't fit at the edge. */
function wideScreen(): Frame[] {
  const s = new Screen(20, 6);
  s.write(0, 0, "日本語のテキスト");
  s.write(0, 1, "a😀b👩‍💻c🇯🇵d");
  s.write(0, 2, "é ñ ä", style(0xe5e510));
  s.write(0, 3, "x日y本z", style(FG, 0x202040));
  // Column 19 is the last: a wide character there leaves a blank spacer.
  s.write(19, 4, "日");
  s.write(0, 5, "wide bold 太字", style(FG, null, Flag.BOLD));
  s.cursor = { ...s.cursor, visible: false };
  return [buildFrame(1, s, null)];
}

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

/** A recording from `dev/recordings`, the frames only. */
function recording(name: string): Frame[] {
  return readFileSync(`${RECORDINGS}${name}.jsonl`, "utf8")
    .split("\n")
    .filter((l) => l.trim())
    .map((l) => (JSON.parse(l) as { frame: Frame }).frame);
}

/** Each input and the metric sets it's answered at. The recordings are
 * real programs with partial frames (the frame → grid step); the metric
 * sets matter for geometry, which the synthetic screens cover. */
const INPUTS: Array<{ name: string; frames: () => Frame[]; metrics: string[] }> = [
  { name: "box", frames: boxScreen, metrics: ["1x", "2x", "odd"] },
  { name: "styles", frames: stylesScreen, metrics: ["1x", "2x", "odd"] },
  { name: "wide", frames: wideScreen, metrics: ["1x", "2x", "odd"] },
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
    files.set(`inputs/${input.name}.jsonl`, frames.map((f) => JSON.stringify(f)).join("\n") + "\n");
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
      for (const dir of ["inputs", "expected"]) rmSync(ROOT + dir, { recursive: true, force: true });
      for (const [path, text] of files) {
        mkdirSync(ROOT + path.slice(0, path.lastIndexOf("/") + 1), { recursive: true });
        writeFileSync(ROOT + path, text);
      }
    });
    return;
  }

  it("has no files the painter doesn't make", () => {
    const stale = [...listed("inputs"), ...listed("expected")].filter((p) => !files.has(p));
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
