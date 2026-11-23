// A pane's picture made smaller: the sidebar's preview and the thumbnails
// parts ask for.

interface Size {
  width: number;
  height: number;
}

/** Where the cursor is in `grid`, as a fraction of its height (the bottom
 * of its row); the bottom when there are no rows. */
export function cursorAt(grid: { rows: number; cursor: { y: number } }): number {
  return grid.rows > 0 ? (grid.cursor.y + 1) / grid.rows : 1;
}

/** The part of `src` that fits `dst` at `scale`: its left part when it's
 * wider, and when it's taller the rows around `cursor` (a fraction of its
 * height), where the activity is: a prompt, an agent's input box. */
export function crop(src: Size, dst: Size, scale: number, cursor: number): { sy: number; sw: number; sh: number } {
  const sw = Math.min(src.width, dst.width / scale);
  const sh = Math.min(src.height, dst.height / scale);
  const sy = Math.max(0, Math.min(src.height - sh, cursor * src.height - sh * 0.7));
  return { sy, sw, sh };
}

/** Draw `src` (a pane's grid canvas) into `ctx` at `scale`, cropped as
 * [`crop`] says, from the top left. */
export function drawThumbnail(ctx: CanvasRenderingContext2D, src: HTMLCanvasElement, scale: number, cursor: number): void {
  const { sy, sw, sh } = crop(src, ctx.canvas, scale, cursor);
  ctx.imageSmoothingQuality = "high";
  ctx.drawImage(src, 0, sy, sw, sh, 0, 0, sw * scale, sh * scale);
}
