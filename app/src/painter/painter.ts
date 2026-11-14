import type { Grid } from "../grid";
import type { Metrics } from "../metrics";
import { css } from "./color";
import { drawOps } from "./draw";
import { planRow, type PaintOp } from "./plan";
import { TextContext } from "./text";

/**
 * Paints the grid mirror onto a canvas, one row at a time: `planRow` decides
 * what to draw (backgrounds, then glyphs, then decorations) and `drawOps`
 * draws it, each row clipped to its own rectangle.
 */
export class Painter {
  readonly ctx: CanvasRenderingContext2D;
  readonly text: TextContext;
  cols = 0;
  rows = 0;
  /** Reused for each row's display list. */
  private ops: PaintOp[] = [];

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
    const ops = this.ops;
    ops.length = 0;
    planRow(grid, y, this.cols, this.m, ops);
    ctx.save();
    ctx.beginPath();
    ctx.rect(0, y * cellH, this.cols * cellW, cellH);
    ctx.clip();
    drawOps(ctx, this.text, ops);
    ctx.restore();
    this.text.invalidate();
    ops.length = 0;
  }
}
