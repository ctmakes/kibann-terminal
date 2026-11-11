import { Flag } from "../proto";
import { fontString, type Metrics } from "../metrics";
import { boxOps, drawBoxOps, isBoxChar } from "./boxdraw";

/**
 * Wraps a 2D context: font selection without redundant changes, cached
 * measurements, and per-cell glyph drawing.
 *
 * A glyph atlas (drawImage from cached glyphs) and run batching with
 * letterSpacing were both measured and rejected; see plans/renderer.md.
 */
export class TextContext {
  private font = "";
  private fonts: string[] = [];
  private widths = new Map<string, number>();

  constructor(
    readonly ctx: CanvasRenderingContext2D,
    public m: Metrics,
  ) {
    this.setMetrics(m);
  }

  setMetrics(m: Metrics): void {
    this.m = m;
    this.fonts = [
      fontString(m, false, false),
      fontString(m, true, false),
      fontString(m, false, true),
      fontString(m, true, true),
    ];
    this.font = "";
    this.widths.clear();
  }

  /** Call after anything that may reset context state (canvas resize, restore). */
  invalidate(): void {
    this.font = "";
  }

  useFont(flags: number): void {
    const i = (flags & Flag.BOLD ? 1 : 0) | (flags & Flag.ITALIC ? 2 : 0);
    const f = this.fonts[i];
    if (f !== this.font) {
      this.ctx.font = f;
      this.ctx.textBaseline = "alphabetic";
      this.font = f;
    }
  }

  measure(text: string): number {
    const key = this.font + "\u0000" + text;
    let w = this.widths.get(key);
    if (w === undefined) {
      w = this.ctx.measureText(text).width;
      if (this.widths.size > 8192) this.widths.clear();
      this.widths.set(key, w);
    }
    return w;
  }

  /**
   * Draw one cell's grapheme at column `col` of the row whose top is `top`,
   * in color `fg`. The font must already be selected with useFont. `bg` is
   * used for the gaps in double box-drawing lines.
   */
  drawGlyph(text: string, col: number, top: number, cells: number, fg: string, bg: string): void {
    const { cellW, cellH, baseline, lineW } = this.m;
    const ctx = this.ctx;
    const x = col * cellW;
    const cp = text.codePointAt(0)!;
    if (text.length === 1 && isBoxChar(cp)) {
      const ops = boxOps(cp, cellW, cellH, lineW);
      if (ops) {
        drawBoxOps(ctx, ops, x, top, cellW, cellH, fg, bg);
        return;
      }
    }
    ctx.fillStyle = fg;
    if (text.length === 1 && cp < 0x7f) {
      ctx.fillText(text, x, top + baseline);
      return;
    }
    const avail = cells * cellW;
    const w = this.measure(text);
    if (w > avail + 0.5) {
      // Glyphs wider than their cells are clipped, not scaled.
      ctx.save();
      ctx.beginPath();
      ctx.rect(x, top, avail, cellH);
      ctx.clip();
      ctx.fillText(text, x, top + baseline);
      ctx.restore();
    } else {
      ctx.fillText(text, x + (cells > 1 && w < avail ? (avail - w) / 2 : 0), top + baseline);
    }
  }
}
