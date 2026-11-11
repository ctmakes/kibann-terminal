import { Flag } from "../proto";
import type { Grid } from "../grid";
import type { Metrics } from "../metrics";
import { css } from "./color";
import { DECORATION_FLAGS, drawDecorations } from "./decorations";
import { TextContext } from "./text";

/**
 * Paints the grid mirror onto a canvas, one row at a time. Backgrounds go
 * first, then glyphs, then decorations, each row clipped to its own rectangle.
 */
export class Painter {
  readonly ctx: CanvasRenderingContext2D;
  readonly text: TextContext;
  cols = 0;
  rows = 0;

  constructor(
    readonly canvas: HTMLCanvasElement,
    public m: Metrics,
  ) {
    const ctx = canvas.getContext("2d", { alpha: false });
    if (!ctx) throw new Error("2D canvas unavailable");
    this.ctx = ctx;
    this.text = new TextContext(ctx, m);
  }

  setMetrics(m: Metrics): void {
    this.m = m;
    this.text.setMetrics(m);
  }

  /** Resize the backing store. This clears the canvas, so paint everything after. */
  resize(cols: number, rows: number): void {
    this.cols = cols;
    this.rows = rows;
    const w = Math.max(1, cols * this.m.cellW);
    const h = Math.max(1, rows * this.m.cellH);
    if (this.canvas.width !== w) this.canvas.width = w;
    if (this.canvas.height !== h) this.canvas.height = h;
    this.canvas.style.width = `${w / this.m.dpr}px`;
    this.canvas.style.height = `${h / this.m.dpr}px`;
    this.text.invalidate();
  }

  paintAll(grid: Grid): void {
    this.ctx.fillStyle = css(grid.defaultBg);
    this.ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);
    for (let y = 0; y < Math.min(grid.rows, this.rows); y++) this.paintRow(grid, y);
  }

  paintRows(grid: Grid, rows: Iterable<number>): void {
    for (const y of rows) if (y < this.rows) this.paintRow(grid, y);
  }

  paintRow(grid: Grid, y: number): void {
    const { cellW, cellH } = this.m;
    const ctx = this.ctx;
    const tc = this.text;
    const top = y * cellH;
    const row = grid.lines[y] ?? grid.blankRow();
    const defaultBg = css(grid.defaultBg);
    const width = this.cols * cellW;

    ctx.save();
    ctx.beginPath();
    ctx.rect(0, top, width, cellH);
    ctx.clip();

    // 1. Backgrounds. Cells past the mirror's width get the default bg.
    ctx.fillStyle = defaultBg;
    ctx.fillRect(0, top, width, cellH);
    for (const r of row) {
      if (r.style.bg === null) continue;
      ctx.fillStyle = css(r.style.bg);
      ctx.fillRect(r.x * cellW, top, r.len * cellW, cellH);
    }

    // 2. Glyphs.
    for (const r of row) {
      if (!r.text || r.style.flags & Flag.INVISIBLE) continue;
      const fg = css(r.style.fg);
      const bg = r.style.bg === null ? defaultBg : css(r.style.bg);
      tc.useFont(r.style.flags);
      ctx.globalAlpha = r.style.flags & Flag.FAINT ? 0.5 : 1;
      ctx.fillStyle = fg;
      for (let i = 0; i < r.len; i++) {
        const t = r.text[i];
        if (!t || t === " ") continue;
        tc.drawGlyph(t, r.x + i, top, r.wide?.has(i) ? 2 : 1, fg, bg);
      }
    }

    // 3. Decorations.
    for (const r of row) {
      const f = r.style.flags;
      if (!(f & DECORATION_FLAGS) || f & Flag.INVISIBLE) continue;
      ctx.globalAlpha = f & Flag.FAINT ? 0.5 : 1;
      const fg = css(r.style.fg);
      drawDecorations(ctx, this.m, f, r.x * cellW, top, r.len * cellW, fg, r.style.ul === null ? fg : css(r.style.ul));
    }

    ctx.restore();
    tc.invalidate();
  }
}
