import { Flag } from "../proto";
import { fontString, type Metrics } from "../metrics";

/**
 * Wraps a 2D context: font selection without redundant changes, cached
 * measurements, and per-cell glyph drawing.
 *
 * A glyph atlas (drawImage from cached glyphs) and run batching with
 * letterSpacing were both measured and rejected; see plans/done/renderer.md.
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
    this.useFontIndex((flags & Flag.BOLD ? 1 : 0) | (flags & Flag.ITALIC ? 2 : 0));
  }

  /** 0 regular, 1 bold, 2 italic, 3 bold italic (see `fontIndex`). */
  useFontIndex(i: number): void {
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
   * Draw a grapheme in `cells` cells whose left edge is `x` and top is
   * `top`, with the current font and fill. Wider than its cells: clipped,
   * not scaled. Narrower than two cells: centered.
   */
  drawText(text: string, x: number, top: number, cells: number): void {
    const { cellW, cellH, baseline } = this.m;
    const ctx = this.ctx;
    if (text.length === 1 && text.charCodeAt(0) < 0x7f) {
      ctx.fillText(text, x, top + baseline);
      return;
    }
    const avail = cells * cellW;
    const w = this.measure(text);
    if (w > avail + 0.5) {
      ctx.save();
      ctx.beginPath();
      ctx.rect(x, top, avail, cellH);
      ctx.clip();
      ctx.fillText(text, x, top + baseline);
      ctx.restore();
      this.invalidate();
    } else {
      ctx.fillText(text, x + (cells > 1 && w < avail ? (avail - w) / 2 : 0), top + baseline);
    }
  }
}
