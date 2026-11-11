import { Flag } from "./proto";
import type { Grid } from "./grid";
import type { Metrics } from "./metrics";
import { css } from "./painter/color";
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
    const c = grid.cursor;
    if (c.x > 0 && grid.cellAt(c.x, c.y).width === 0) return c.x - 1;
    return c.x;
  }

  draw(): void {
    const ctx = this.ctx;
    ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
    const grid = this.grid;
    if (!grid) return;
    const c = grid.cursor;
    if (!c.in_viewport || c.y >= this.rows) return;
    const { cellW, cellH, lineW } = this.m;
    const col = this.cursorColumn(grid);
    const top = c.y * cellH;
    const x = col * cellW;

    if (this.preedit) {
      this.drawPreedit(grid, col, top);
      return;
    }
    if (!c.visible) return;
    if (this.focused && c.blinking && !this.blinkOn) return;

    const color = css(c.color ?? grid.defaultFg);
    const cell = grid.cellAt(col, c.y);
    const cells = c.wide || cell.width === 2 ? 2 : 1;
    const w = Math.min(cells, Math.max(1, this.cols - col)) * cellW;
    const style = this.focused ? c.style : "block_hollow";

    switch (style) {
      case "block": {
        ctx.fillStyle = color;
        ctx.fillRect(x, top, w, cellH);
        if (cell.text && cell.text !== " " && !(cell.style.flags & Flag.INVISIBLE)) {
          const textColor = css(cell.style.bg ?? grid.defaultBg);
          this.text.invalidate();
          this.text.useFont(cell.style.flags);
          ctx.fillStyle = textColor;
          this.text.drawGlyph(cell.text, col, top, cells, textColor, color);
        }
        break;
      }
      case "block_hollow":
        ctx.strokeStyle = color;
        ctx.lineWidth = lineW;
        ctx.strokeRect(x + lineW / 2, top + lineW / 2, w - lineW, cellH - lineW);
        break;
      case "bar":
        ctx.fillStyle = color;
        ctx.fillRect(x, top, Math.max(lineW, Math.round(cellW / 8)), cellH);
        break;
      case "underline": {
        const th = Math.max(lineW, Math.round(cellH / 14));
        ctx.fillStyle = color;
        ctx.fillRect(x, top + cellH - th, w, th);
        break;
      }
    }
  }

  private drawPreedit(grid: Grid, col: number, top: number): void {
    const ctx = this.ctx;
    const { cellW, cellH, lineW, underlineY } = this.m;
    const fg = css(grid.defaultFg);
    const bg = css(grid.defaultBg);
    const parts = graphemes(this.preedit).map((g) => [g, Math.max(1, graphemeWidth(g))] as const);
    const total = parts.reduce((n, [, w]) => n + w, 0);
    // Long compositions start further left so the end, where typing happens, stays visible.
    const start = Math.max(0, Math.min(col, this.cols - total));
    ctx.fillStyle = bg;
    ctx.fillRect(start * cellW, top, Math.min(total, this.cols - start) * cellW, cellH);
    this.text.invalidate();
    this.text.useFont(0);
    ctx.fillStyle = fg;
    let x = start;
    for (const [g, w] of parts) {
      if (x + w > this.cols) break;
      this.text.drawGlyph(g, x, top, w, fg, bg);
      x += w;
    }
    ctx.fillStyle = fg;
    ctx.fillRect(start * cellW, top + underlineY, (x - start) * cellW, lineW);
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
