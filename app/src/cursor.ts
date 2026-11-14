import type { Grid } from "./grid";
import type { Metrics } from "./metrics";
import { drawOps } from "./painter/draw";
import { cursorColumn, planCursor, planGlyph, type PaintOp } from "./painter/plan";
import { TextContext } from "./painter/text";
import { graphemes, graphemeWidth } from "./wcwidth";

export const BLINK_MS = 600;

/**
 * Transparent canvas over the grid. Holds the cursor and IME preedit text so
 * blinking and composing never repaint grid rows.
 */
export class CursorLayer {
  readonly ctx: CanvasRenderingContext2D;
  readonly text: TextContext;
  private grid: Grid | null = null;
  private focused = false;
  private preedit = "";
  private blinkOn = true;
  private timer: ReturnType<typeof setInterval> | null = null;
  private cols = 0;
  private rows = 0;

  constructor(
    readonly canvas: HTMLCanvasElement,
    public m: Metrics,
  ) {
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("2D canvas unavailable");
    this.ctx = ctx;
    this.text = new TextContext(ctx, m);
  }

  setMetrics(m: Metrics): void {
    this.m = m;
    this.text.setMetrics(m);
  }

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

  setFocused(focused: boolean): void {
    this.focused = focused;
    this.resetBlink();
  }

  setPreedit(text: string): void {
    this.preedit = text;
    this.draw();
  }

  /** Show the cursor solid and restart the blink cycle, e.g. after input. */
  resetBlink(): void {
    this.blinkOn = true;
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    if (this.focused && this.grid?.cursor.blinking) {
      this.timer = setInterval(() => {
        this.blinkOn = !this.blinkOn;
        this.draw();
      }, BLINK_MS);
    }
    this.draw();
  }

  update(grid: Grid, cursorChanged: boolean): void {
    const wasBlinking = this.grid?.cursor.blinking ?? false;
    this.grid = grid;
    if (cursorChanged || wasBlinking !== grid.cursor.blinking) this.resetBlink();
    else this.draw();
  }

  /** Column of the cell the cursor is drawn on, moved off a wide tail. */
  cursorColumn(grid: Grid): number {
    return cursorColumn(grid);
  }

  draw(): void {
    const ctx = this.ctx;
    ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
    const grid = this.grid;
    if (!grid) return;
    const c = grid.cursor;
    if (!c.in_viewport || c.y >= this.rows) return;
    const col = this.cursorColumn(grid);

    if (this.preedit) {
      this.drawPreedit(grid, col, c.y);
      return;
    }
    if (!c.visible) return;
    if (this.focused && c.blinking && !this.blinkOn) return;

    const style = this.focused ? c.style : "block_hollow";
    this.text.invalidate();
    drawOps(ctx, this.text, planCursor(grid, col, c.y, this.cols, style, this.m));
  }

  /** IME text being composed, at the cursor, underlined. */
  private drawPreedit(grid: Grid, col: number, y: number): void {
    const { cellW, cellH, lineW, underlineY } = this.m;
    const top = y * cellH;
    const fg = grid.defaultFg;
    const bg = grid.defaultBg;
    const parts = graphemes(this.preedit).map((g) => [g, Math.max(1, graphemeWidth(g))] as const);
    const total = parts.reduce((n, [, w]) => n + w, 0);
    // Long compositions start further left so the end, where typing happens, stays visible.
    const start = Math.max(0, Math.min(col, this.cols - total));
    const ops: PaintOp[] = [
      { k: "fill", x: start * cellW, y: top, w: Math.min(total, this.cols - start) * cellW, h: cellH, color: bg, alpha: 1 },
    ];
    let x = start;
    for (const [g, w] of parts) {
      if (x + w > this.cols) break;
      planGlyph(ops, this.m, g, x * cellW, top, w, 0, fg, bg, 1);
      x += w;
    }
    ops.push({ k: "fill", x: start * cellW, y: top + underlineY, w: (x - start) * cellW, h: lineW, color: fg, alpha: 1 });
    this.text.invalidate();
    drawOps(this.ctx, this.text, ops);
  }

  /** CSS-pixel position of the cursor cell, relative to the grid's origin. */
  cursorCss(grid: Grid): { left: number; top: number; height: number } {
    const col = this.cursorColumn(grid);
    return {
      left: (col * this.m.cellW) / this.m.dpr,
      top: (grid.cursor.y * this.m.cellH) / this.m.dpr,
      height: this.m.cellH / this.m.dpr,
    };
  }

  dispose(): void {
    if (this.timer) clearInterval(this.timer);
  }
}
