import type { Metrics } from "./metrics";

/** Columns and rows that fit in a container of the given CSS size. */
export function gridSize(widthCss: number, heightCss: number, m: Metrics): { cols: number; rows: number } {
  const w = Math.floor(widthCss * m.dpr) - 2 * m.pad;
  const h = Math.floor(heightCss * m.dpr) - 2 * m.pad;
  return {
    cols: Math.max(2, Math.floor(w / m.cellW)),
    rows: Math.max(1, Math.floor(h / m.cellH)),
  };
}

/** ResizeObserver, coalesced to one callback per animation frame. */
export function watchSize(el: HTMLElement, cb: (widthCss: number, heightCss: number) => void): () => void {
  let pending = 0;
  const ro = new ResizeObserver(() => {
    if (pending) return;
    pending = requestAnimationFrame(() => {
      pending = 0;
      cb(el.clientWidth, el.clientHeight);
    });
  });
  ro.observe(el);
  return () => {
    ro.disconnect();
    if (pending) cancelAnimationFrame(pending);
  };
}
