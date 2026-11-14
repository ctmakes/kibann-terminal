// The deciding half of the painter: grid rows and the cursor become a
// display list, plain data with no canvas. `draw.ts` paints it. Keeping the
// two apart lets another painter (the phone app's) be checked against the
// same lists, and lets the logic be tested without a canvas.
//
// Everything is in device pixels for the given metrics; colors are 0xRRGGBB.

import { Flag, Underline, underlineOf, type CursorStyle } from "../proto";
import type { Grid } from "../grid";
import type { Metrics } from "../metrics";
import { boxOps, isBoxChar, type Op } from "./boxdraw";

export type PaintOp =
  /** A filled rectangle: backgrounds, decorations, cursors. */
  | { k: "fill"; x: number; y: number; w: number; h: number; color: number; alpha: number }
  /** A grapheme in `cells` cells whose left edge is `x` and top is `y`.
   * `font`: 0 regular, 1 bold, 2 italic, 3 bold italic. Placing it inside
   * its cells (measuring, centering, clipping) is the drawing half's job. */
  | { k: "glyph"; x: number; y: number; text: string; cells: number; font: number; fg: number; alpha: number }
  /** A box-drawing, block, braille or Powerline character as geometry
   * (`ops` relative to the cell's top-left); `bg` fills double-line gaps. */
  | { k: "box"; x: number; y: number; w: number; h: number; ops: Op[]; fg: number; bg: number; alpha: number }
  /** A curly underline: one wave per cell from `x` across `w`, centered on
   * `mid`, clipped to the row (`y`, `h`). */
  | { k: "wave"; x: number; y: number; w: number; h: number; mid: number; amp: number; cellW: number; lw: number; color: number; alpha: number }
  /** An outlined rectangle (the hollow cursor), the line inside the edges. */
  | { k: "frame"; x: number; y: number; w: number; h: number; lw: number; color: number; alpha: number };

export const DECORATION_FLAGS = Flag.UNDERLINE_MASK | Flag.STRIKE | Flag.OVERLINE;

/** The font for a style's flags: 0 regular, 1 bold, 2 italic, 3 both. */
export function fontIndex(flags: number): number {
  return (flags & Flag.BOLD ? 1 : 0) | (flags & Flag.ITALIC ? 2 : 0);
}

/** One grapheme: geometry for box characters, else text. */
export function planGlyph(
  out: PaintOp[],
  m: Metrics,
  text: string,
  x: number,
  y: number,
  cells: number,
  font: number,
  fg: number,
  bg: number,
  alpha: number,
): void {
  const { cellW, cellH, lineW } = m;
  if (text.length === 1) {
    const cp = text.codePointAt(0)!;
    if (isBoxChar(cp)) {
      const ops = boxOps(cp, cellW, cellH, lineW);
      if (ops) {
        out.push({ k: "box", x, y, w: cellW, h: cellH, ops, fg, bg, alpha });
        return;
      }
    }
  }
  out.push({ k: "glyph", x, y, text, cells, font, fg, alpha });
}

/**
 * Row `y` of `grid`, `cols` cells wide: backgrounds first, then glyphs,
 * then decorations. Appends to `out` and returns it.
 */
export function planRow(grid: Grid, y: number, cols: number, m: Metrics, out: PaintOp[] = []): PaintOp[] {
  const { cellW, cellH } = m;
  const top = y * cellH;
  const row = grid.lines[y] ?? grid.blankRow();
  const defaultBg = grid.defaultBg;

  // 1. Backgrounds. Cells past the mirror's width get the default bg.
  out.push({ k: "fill", x: 0, y: top, w: cols * cellW, h: cellH, color: defaultBg, alpha: 1 });
  for (const r of row) {
    if (r.style.bg === null) continue;
    out.push({ k: "fill", x: r.x * cellW, y: top, w: r.len * cellW, h: cellH, color: r.style.bg, alpha: 1 });
  }

  // 2. Glyphs.
  for (const r of row) {
    const f = r.style.flags;
    if (!r.text || f & Flag.INVISIBLE) continue;
    const fg = r.style.fg;
    const bg = r.style.bg ?? defaultBg;
    const font = fontIndex(f);
    const alpha = f & Flag.FAINT ? 0.5 : 1;
    for (let i = 0; i < r.len; i++) {
      const t = r.text[i];
      if (!t || t === " ") continue;
      planGlyph(out, m, t, (r.x + i) * cellW, top, r.wide?.has(i) ? 2 : 1, font, fg, bg, alpha);
    }
  }

  // 3. Decorations.
  for (const r of row) {
    const f = r.style.flags;
    if (!(f & DECORATION_FLAGS) || f & Flag.INVISIBLE) continue;
    const fg = r.style.fg;
    planDecorations(out, m, f, r.x * cellW, top, r.len * cellW, fg, r.style.ul ?? fg, f & Flag.FAINT ? 0.5 : 1);
  }
  return out;
}

/** Underline (all styles), strikethrough and overline across `width`. */
export function planDecorations(
  out: PaintOp[],
  m: Metrics,
  flags: number,
  x0: number,
  top: number,
  width: number,
  textColor: number,
  ulColor: number,
  alpha: number,
): void {
  const { lineW, underlineY, strikeY, cellH, cellW } = m;
  const rect = (x: number, y: number, w: number, color: number) =>
    out.push({ k: "fill", x, y, w, h: lineW, color, alpha });
  const ul = underlineOf(flags);
  if (ul !== Underline.NONE) {
    const y = top + underlineY;
    switch (ul) {
      case Underline.SINGLE:
        rect(x0, y, width, ulColor);
        break;
      case Underline.DOUBLE:
        rect(x0, y, width, ulColor);
        rect(x0, Math.min(y + 2 * lineW, top + cellH - lineW), width, ulColor);
        break;
      case Underline.DOTTED: {
        const step = Math.max(2, 2 * lineW);
        for (let x = x0; x < x0 + width; x += step) {
          out.push({ k: "fill", x, y, w: Math.min(lineW, x0 + width - x), h: lineW, color: ulColor, alpha });
        }
        break;
      }
      case Underline.DASHED: {
        const dash = Math.max(2, Math.round(cellW / 2));
        for (let x = x0; x < x0 + width; x += 2 * dash) {
          out.push({ k: "fill", x, y, w: Math.min(dash, x0 + width - x), h: lineW, color: ulColor, alpha });
        }
        break;
      }
      case Underline.CURLY: {
        // One wave per cell, phase-locked to the grid so adjacent runs line up.
        const amp = Math.max(1, Math.min(lineW * 1.5, (cellH - underlineY) / 2 - lineW / 2));
        out.push({ k: "wave", x: x0, y: top, w: width, h: cellH, mid: y + amp, amp, cellW, lw: lineW, color: ulColor, alpha });
        break;
      }
    }
  }
  if (flags & Flag.STRIKE) rect(x0, top + strikeY, width, textColor);
  if (flags & Flag.OVERLINE) rect(x0, top, width, textColor);
}

/**
 * The cursor drawn on column `col` of row `y` (already moved off a wide
 * character's tail) in `style`, at most `cols` wide. Blinking, focus and
 * composing text are the caller's: this is only what one cursor looks like.
 */
export function planCursor(
  grid: Grid,
  col: number,
  y: number,
  cols: number,
  style: CursorStyle,
  m: Metrics,
  out: PaintOp[] = [],
): PaintOp[] {
  const { cellW, cellH, lineW } = m;
  const c = grid.cursor;
  const color = c.color ?? grid.defaultFg;
  const cell = grid.cellAt(col, y);
  const cells = c.wide || cell.width === 2 ? 2 : 1;
  const w = Math.min(cells, Math.max(1, cols - col)) * cellW;
  const x = col * cellW;
  const top = y * cellH;
  switch (style) {
    case "block":
      out.push({ k: "fill", x, y: top, w, h: cellH, color, alpha: 1 });
      if (cell.text && cell.text !== " " && !(cell.style.flags & Flag.INVISIBLE)) {
        const textColor = cell.style.bg ?? grid.defaultBg;
        planGlyph(out, m, cell.text, x, top, cells, fontIndex(cell.style.flags), textColor, color, 1);
      }
      break;
    case "block_hollow":
      out.push({ k: "frame", x, y: top, w, h: cellH, lw: lineW, color, alpha: 1 });
      break;
    case "bar":
      out.push({ k: "fill", x, y: top, w: Math.max(lineW, Math.round(cellW / 8)), h: cellH, color, alpha: 1 });
      break;
    case "underline": {
      const th = Math.max(lineW, Math.round(cellH / 14));
      out.push({ k: "fill", x, y: top + cellH - th, w, h: th, color, alpha: 1 });
      break;
    }
  }
  return out;
}
