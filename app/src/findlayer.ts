import type { Grid } from "./grid";
import type { Metrics } from "./metrics";
import type { Find } from "./proto";
import "./terminal.css";

/** Search matches over the text: every match in view tinted, the selected
 * one stronger and outlined. DOM like the link layer: a few boxes that come
 * and go. */
export class FindLayer {
  readonly el: HTMLDivElement;
  private shown: Find | null = null;

  constructor(gridEl: HTMLElement) {
    this.el = document.createElement("div");
    this.el.className = "term-find";
    gridEl.append(this.el);
  }

  /** Show `grid.find`'s spans. `force` redraws after the metrics changed. */
  update(grid: Grid, m: Metrics, force = false): void {
    const find = grid.find;
    if (find === this.shown && !force) return;
    this.shown = find;
    this.el.replaceChildren(
      ...(find?.spans ?? []).map((s) => {
        const box = document.createElement("div");
        box.className = s.selected ? "term-find-match term-find-selected" : "term-find-match";
        Object.assign(box.style, {
          left: `${(s.x * m.cellW) / m.dpr}px`,
          top: `${(s.y * m.cellH) / m.dpr}px`,
          width: `${(s.len * m.cellW) / m.dpr}px`,
          height: `${m.cellH / m.dpr}px`,
        });
        return box;
      }),
    );
  }
}
