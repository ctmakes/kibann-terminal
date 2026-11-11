import type { Cursor, Frame, MouseTracking, Row, Style } from "./proto";

/** A run with its style resolved out of the frame's style table. */
export interface GridRun {
  x: number;
  len: number;
  style: Style;
  /** `len` entries, or null when every cell is blank. */
  text: string[] | null;
  /** Offsets within the run that hold a wide character, or null. */
  wide: Set<number> | null;
}

export type GridRow = GridRun[];

export interface CellView {
  text: string;
  style: Style;
  /** 1 narrow, 2 wide (head), 0 tail of a wide character. */
  width: 0 | 1 | 2;
}

export type ApplyResult =
  | { applied: true; full: boolean; resized: boolean; dirty: Set<number> }
  | { applied: false; requestFull: boolean };

/**
 * The frontend's mirror of the backend screen. Frames are applied in order;
 * a gap in `seq` drops partial frames until a full one arrives.
 */
export class Grid {
  cols = 0;
  rows = 0;
  defaultFg = 0xd8d8d8;
  defaultBg = 0x181818;
  cursor: Cursor = {
    x: 0,
    y: 0,
    in_viewport: false,
    visible: false,
    blinking: false,
    style: "block",
    wide: false,
    color: null,
  };
  mouseTracking: MouseTracking = "none";
  focusReporting = false;
  lines: GridRow[] = [];
  lastSeq: number | null = null;
  awaitingFull = false;

  defaultStyle(): Style {
    return { fg: this.defaultFg, bg: null, ul: null, flags: 0 };
  }

  blankRow(cols = this.cols, style = this.defaultStyle()): GridRow {
    return cols > 0 ? [{ x: 0, len: cols, style, text: null, wide: null }] : [];
  }

  apply(frame: Frame): ApplyResult {
    if (!frame.full) {
      const inOrder =
        this.lastSeq !== null &&
        frame.seq === this.lastSeq + 1 &&
        frame.cols === this.cols &&
        frame.rows_total === this.rows;
      if (this.awaitingFull) return { applied: false, requestFull: false };
      if (!inOrder) {
        this.awaitingFull = true;
        return { applied: false, requestFull: true };
      }
    }

    const resized = frame.cols !== this.cols || frame.rows_total !== this.rows;
    const bgChanged = frame.default_bg !== this.defaultBg;
    this.cols = frame.cols;
    this.rows = frame.rows_total;
    this.defaultFg = frame.default_fg;
    this.defaultBg = frame.default_bg;
    this.cursor = frame.cursor;
    this.mouseTracking = frame.mouse_tracking;
    this.focusReporting = frame.focus_reporting;
    this.lastSeq = frame.seq;

    const dirty = new Set<number>();
    if (frame.full) {
      this.awaitingFull = false;
      this.lines = Array.from({ length: this.rows }, () => this.blankRow());
      for (let y = 0; y < this.rows; y++) dirty.add(y);
    } else if (bgChanged) {
      // Null backgrounds mean "default", so every row looks different now.
      for (let y = 0; y < this.rows; y++) dirty.add(y);
    }

    for (const row of frame.rows) {
      if (row.y >= this.rows) continue;
      this.lines[row.y] = this.resolveRow(row, frame.styles);
      dirty.add(row.y);
    }
    return { applied: true, full: frame.full, resized, dirty };
  }

  private resolveRow(row: Row, styles: Style[]): GridRow {
    const out: GridRow = [];
    let next = 0;
    for (const r of row.runs) {
      if (r.x !== next || r.len <= 0 || next >= this.cols) break;
      const len = Math.min(r.len, this.cols - r.x);
      const style = styles[r.s] ?? this.defaultStyle();
      const text = r.text ? r.text.slice(0, len) : null;
      const wide = r.wide && r.wide.length ? new Set(r.wide.filter((o) => o < len)) : null;
      out.push({ x: r.x, len, style, text, wide });
      next = r.x + len;
    }
    if (next < this.cols) {
      out.push({ x: next, len: this.cols - next, style: this.defaultStyle(), text: null, wide: null });
    }
    return out;
  }

  /**
   * Crop or pad the mirror to a new size so the painter has something sensible
   * to show until the backend's full frame arrives.
   */
  resizeLocal(cols: number, rows: number): void {
    if (cols === this.cols && rows === this.rows) return;
    const lines: GridRow[] = [];
    for (let y = 0; y < rows; y++) {
      const src = this.lines[y];
      if (!src) {
        lines.push(this.blankRow(cols));
        continue;
      }
      const row: GridRow = [];
      for (const r of src) {
        if (r.x >= cols) break;
        let len = Math.min(r.len, cols - r.x);
        let text = r.text ? r.text.slice(0, len) : null;
        let wide = r.wide ? new Set([...r.wide].filter((o) => o < len)) : null;
        // A wide head with its tail cut off would overflow the grid.
        if (text && wide && wide.has(len - 1)) {
          wide.delete(len - 1);
          text[len - 1] = "";
        }
        if (wide && wide.size === 0) wide = null;
        row.push({ x: r.x, len, style: r.style, text, wide });
      }
      const covered = row.length ? row[row.length - 1].x + row[row.length - 1].len : 0;
      if (covered < cols) {
        row.push({ x: covered, len: cols - covered, style: this.defaultStyle(), text: null, wide: null });
      }
      lines.push(row);
    }
    this.lines = lines;
    this.cols = cols;
    this.rows = rows;
    if (this.cursor.x >= cols || this.cursor.y >= rows) {
      this.cursor = { ...this.cursor, in_viewport: false };
    }
  }

  cellAt(x: number, y: number): CellView {
    const row = this.lines[y];
    if (row) {
      for (const r of row) {
        if (x < r.x || x >= r.x + r.len) continue;
        const o = x - r.x;
        const text = r.text ? r.text[o] ?? "" : "";
        let width: 0 | 1 | 2 = 1;
        if (r.wide?.has(o)) width = 2;
        else if (o > 0 && r.wide?.has(o - 1)) width = 0;
        else if (o === 0 && x > 0 && this.isWideHeadAt(row, x - 1)) width = 0;
        return { text, style: r.style, width };
      }
    }
    return { text: "", style: this.defaultStyle(), width: 1 };
  }

  private isWideHeadAt(row: GridRow, x: number): boolean {
    for (const r of row) {
      if (x >= r.x && x < r.x + r.len) return r.wide?.has(x - r.x) ?? false;
    }
    return false;
  }
}
