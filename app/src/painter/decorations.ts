import { Flag, Underline, underlineOf } from "../proto";
import type { Metrics } from "../metrics";

export const DECORATION_FLAGS = Flag.UNDERLINE_MASK | Flag.STRIKE | Flag.OVERLINE;

/** Underline (all styles), strikethrough and overline across `cells` cells. */
export function drawDecorations(
  ctx: CanvasRenderingContext2D,
  m: Metrics,
  flags: number,
  x0: number,
  top: number,
  width: number,
  textColor: string,
  ulColor: string,
): void {
  const { lineW, underlineY, strikeY, cellH, cellW } = m;
  const ul = underlineOf(flags);
  if (ul !== Underline.NONE) {
    ctx.fillStyle = ulColor;
    const y = top + underlineY;
    switch (ul) {
      case Underline.SINGLE:
        ctx.fillRect(x0, y, width, lineW);
        break;
      case Underline.DOUBLE:
        ctx.fillRect(x0, y, width, lineW);
        ctx.fillRect(x0, Math.min(y + 2 * lineW, top + cellH - lineW), width, lineW);
        break;
      case Underline.DOTTED: {
        const step = Math.max(2, 2 * lineW);
        for (let x = x0; x < x0 + width; x += step) ctx.fillRect(x, y, Math.min(lineW, x0 + width - x), lineW);
        break;
      }
      case Underline.DASHED: {
        const dash = Math.max(2, Math.round(cellW / 2));
        for (let x = x0; x < x0 + width; x += 2 * dash) ctx.fillRect(x, y, Math.min(dash, x0 + width - x), lineW);
        break;
      }
      case Underline.CURLY: {
        // One wave per cell, phase-locked to the grid so adjacent runs line up.
        const amp = Math.max(1, Math.min(lineW * 1.5, (cellH - underlineY) / 2 - lineW / 2));
        const mid = y + amp;
        ctx.save();
        ctx.beginPath();
        ctx.rect(x0, top, width, cellH);
        ctx.clip();
        ctx.beginPath();
        ctx.moveTo(x0, mid);
        for (let x = x0; x < x0 + width; x += cellW) {
          ctx.bezierCurveTo(x + cellW / 4, mid - 2 * amp, x + cellW / 4, mid - 2 * amp, x + cellW / 2, mid);
          ctx.bezierCurveTo(x + (3 * cellW) / 4, mid + 2 * amp, x + (3 * cellW) / 4, mid + 2 * amp, x + cellW, mid);
        }
        ctx.lineWidth = lineW;
        ctx.strokeStyle = ulColor;
        ctx.stroke();
        ctx.restore();
        break;
      }
    }
  }
  if (flags & Flag.STRIKE) {
    ctx.fillStyle = textColor;
    ctx.fillRect(x0, top + strikeY, width, lineW);
  }
  if (flags & Flag.OVERLINE) {
    ctx.fillStyle = textColor;
    ctx.fillRect(x0, top, width, lineW);
  }
}
