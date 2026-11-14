// The drawing half of the painter: a display list from `plan.ts` onto a 2D
// canvas. Only what depends on the canvas lives here: fonts, measuring and
// placing glyphs, paths and clipping.

import { drawBoxOps } from "./boxdraw";
import { css } from "./color";
import type { PaintOp } from "./plan";
import type { TextContext } from "./text";

export function drawOps(ctx: CanvasRenderingContext2D, text: TextContext, ops: readonly PaintOp[]): void {
  let alpha = ctx.globalAlpha;
  const useAlpha = (a: number) => {
    if (a !== alpha) {
      ctx.globalAlpha = a;
      alpha = a;
    }
  };
  // Setting fillStyle parses the color: skip it while it doesn't change
  // (runs of glyphs share one). Anything else that sets it resets this.
  let fill = -1;
  const useFill = (c: number) => {
    if (c !== fill) {
      ctx.fillStyle = css(c);
      fill = c;
    }
  };
  for (const op of ops) {
    switch (op.k) {
      case "fill":
        useAlpha(op.alpha);
        useFill(op.color);
        ctx.fillRect(op.x, op.y, op.w, op.h);
        break;
      case "glyph":
        useAlpha(op.alpha);
        text.useFontIndex(op.font);
        useFill(op.fg);
        text.drawText(op.text, op.x, op.y, op.cells);
        break;
      case "box":
        useAlpha(op.alpha);
        drawBoxOps(ctx, op.ops, op.x, op.y, op.w, op.h, css(op.fg), css(op.bg));
        fill = -1;
        break;
      case "wave": {
        useAlpha(op.alpha);
        const { x, w, mid, amp, cellW } = op;
        ctx.save();
        ctx.beginPath();
        ctx.rect(x, op.y, w, op.h);
        ctx.clip();
        ctx.beginPath();
        ctx.moveTo(x, mid);
        for (let cx = x; cx < x + w; cx += cellW) {
          ctx.bezierCurveTo(cx + cellW / 4, mid - 2 * amp, cx + cellW / 4, mid - 2 * amp, cx + cellW / 2, mid);
          ctx.bezierCurveTo(cx + (3 * cellW) / 4, mid + 2 * amp, cx + (3 * cellW) / 4, mid + 2 * amp, cx + cellW, mid);
        }
        ctx.lineWidth = op.lw;
        ctx.strokeStyle = css(op.color);
        ctx.stroke();
        ctx.restore();
        text.invalidate();
        fill = -1;
        break;
      }
      case "frame":
        useAlpha(op.alpha);
        ctx.strokeStyle = css(op.color);
        ctx.lineWidth = op.lw;
        ctx.strokeRect(op.x + op.lw / 2, op.y + op.lw / 2, op.w - op.lw, op.h - op.lw);
        break;
    }
  }
}
