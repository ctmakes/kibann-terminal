// A simple screen model for fixtures and tests, plus the diff/RLE that turns
// screens into protocol frames the same way the backend's frame builder will.

import type { Cursor, Frame, MouseTracking, Row, Run, Scrollbar, Style } from "../proto";
import { graphemes, graphemeWidth } from "../wcwidth";

export interface Cell {
  text: string;
  style: Style;
  /** 1 narrow, 2 wide head, 0 wide tail. */
  width: 0 | 1 | 2;
}

export const DEFAULT_FG = 0xd8d8d8;
export const DEFAULT_BG = 0x181818;

export function style(fg: number = DEFAULT_FG, bg: number | null = null, flags = 0, ul: number | null = null): Style {
  return { fg, bg, ul, flags };
}

export class Screen {
  cells: Cell[][];
  cursor: Cursor = { x: 0, y: 0, in_viewport: true, visible: true, blinking: false, style: "block", wide: false, color: null };
  mouseTracking: MouseTracking = "none";
  focusReporting = false;
  /** Null means no scrollback: the viewport is the whole screen. */
  scrollbar: Scrollbar | null = null;

  constructor(
    readonly cols: number,
    readonly rows: number,
    public defaultFg = DEFAULT_FG,
    public defaultBg = DEFAULT_BG,
  ) {
    this.cells = Array.from({ length: rows }, () => this.blankLine());
  }

  blank(): Cell {
    return { text: "", style: style(this.defaultFg), width: 1 };
  }

  blankLine(): Cell[] {
    return Array.from({ length: this.cols }, () => this.blank());
  }

  /** Write text at (x, y), handling wide graphemes. Returns the column after it. */
  write(x: number, y: number, text: string, st: Style = style(this.defaultFg)): number {
    if (y < 0 || y >= this.rows) return x;
    for (const g of graphemes(text)) {
      const w = graphemeWidth(g);
      if (w === 0) continue;
      if (x + w > this.cols) {
        // A wide char that doesn't fit leaves a blank spacer (SPACER_HEAD).
        if (x < this.cols) this.cells[y][x] = { text: "", style: st, width: 1 };
        break;
      }
      this.cells[y][x] = { text: g, style: st, width: w === 2 ? 2 : 1 };
      if (w === 2) this.cells[y][x + 1] = { text: "", style: st, width: 0 };
      x += w;
    }
    return x;
  }

  fill(x: number, y: number, w: number, h: number, st: Style, text = ""): void {
    for (let yy = y; yy < Math.min(this.rows, y + h); yy++) {
      for (let xx = x; xx < Math.min(this.cols, x + w); xx++) {
        this.cells[yy][xx] = { text, style: st, width: 1 };
      }
    }
  }

  clone(): Screen {
    const s = new Screen(this.cols, this.rows, this.defaultFg, this.defaultBg);
    s.cells = this.cells.map((r) => r.map((c) => ({ ...c })));
    s.cursor = { ...this.cursor };
    s.mouseTracking = this.mouseTracking;
    s.focusReporting = this.focusReporting;
    return s;
  }
}

function styleKey(s: Style): string {
  return `${s.fg}|${s.bg}|${s.ul}|${s.flags}`;
}

function cellKey(c: Cell): string {
  return `${c.text}\u0000${c.width}\u0000${styleKey(c.style)}`;
}

function rowChanged(a: Cell[] | undefined, b: Cell[]): boolean {
  if (!a || a.length !== b.length) return true;
  for (let i = 0; i < b.length; i++) if (cellKey(a[i]) !== cellKey(b[i])) return true;
  return false;
}

/** Turn a screen into a frame: full, or only rows that differ from `prev`. */
export function buildFrame(seq: number, next: Screen, prev: Screen | null): Frame {
  const full = !prev || prev.cols !== next.cols || prev.rows !== next.rows || prev.defaultBg !== next.defaultBg;
  const styles: Style[] = [];
  const index = new Map<string, number>();
  const styleIndex = (s: Style) => {
    const k = styleKey(s);
    let i = index.get(k);
    if (i === undefined) {
      i = styles.length;
      styles.push(s);
      index.set(k, i);
    }
    return i;
  };

  const rows: Row[] = [];
  for (let y = 0; y < next.rows; y++) {
    const line = next.cells[y];
    if (!full && !rowChanged(prev!.cells[y], line)) continue;
    const runs: Run[] = [];
    let x = 0;
    while (x < next.cols) {
      const s = styleIndex(line[x].style);
      const k = styleKey(line[x].style);
      let end = x + 1;
      while (end < next.cols && styleKey(line[end].style) === k) end++;
      const cells = line.slice(x, end);
      const run: Run = { x, len: end - x, s };
      if (cells.some((c) => c.text !== "")) run.text = cells.map((c) => c.text);
      const wide = cells.flatMap((c, i) => (c.width === 2 ? [i] : []));
      if (wide.length) run.wide = wide;
      runs.push(run);
      x = end;
    }
    rows.push({ y, runs });
  }

  return {
    seq,
    full,
    cols: next.cols,
    rows_total: next.rows,
    default_fg: next.defaultFg,
    default_bg: next.defaultBg,
    styles,
    rows,
    cursor: { ...next.cursor },
    mouse_tracking: next.mouseTracking,
    focus_reporting: next.focusReporting,
    scrollbar: next.scrollbar ?? { total: next.rows, offset: 0, len: next.rows },
  };
}
