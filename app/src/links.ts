import type { Grid } from "./grid";
import type { Metrics } from "./metrics";
import type { Link } from "./proto";
import { css } from "./painter/color";

/** Whether viewport cell (`x`, `y`) is part of `link`. */
export function linkCovers(link: Link, x: number, y: number): boolean {
  return link.spans.some((s) => s.y === y && x >= s.x && x < s.x + s.len);
}

/**
 * The hovered link: an underline under each of its spans, and its address in
 * a box at the bottom-left of the pane (Ghostty's link preview). DOM rather
 * than canvas, since there are only a few spans and they come and go.
 */
export class LinkLayer {
  /** Holds the underlines; sits over the grid with the same origin. */
  readonly el: HTMLDivElement;
  /** The address preview; sits in the pane's root. */
  readonly preview: HTMLDivElement;
  private shown: Link | null = null;

  constructor(gridEl: HTMLElement, root: HTMLElement) {
    this.el = document.createElement("div");
    this.el.className = "term-links";
    Object.assign(this.el.style, { position: "absolute", left: "0", top: "0", pointerEvents: "none" });
    gridEl.append(this.el);

    this.preview = document.createElement("div");
    this.preview.className = "term-link-preview";
    Object.assign(this.preview.style, {
      position: "absolute",
      left: "4px",
      bottom: "4px",
      maxWidth: "calc(100% - 8px)",
      padding: "2px 6px",
      borderRadius: "4px",
      font: "12px -apple-system, system-ui, sans-serif",
      whiteSpace: "nowrap",
      overflow: "hidden",
      textOverflow: "ellipsis",
      pointerEvents: "none",
      display: "none",
    });
    root.append(this.preview);
  }

  /** Show `grid.link`, or hide when there is none. `force` redraws after
   * the metrics changed. */
  update(grid: Grid, m: Metrics, force = false): void {
    const link = grid.link;
    if (link === this.shown && !force) return;
    this.shown = link;
    this.el.replaceChildren();
    if (!link) {
      this.preview.style.display = "none";
      return;
    }
    // One device pixel thick, near the bottom of the cell like an underline.
    const thick = Math.max(1, Math.round(m.dpr)) / m.dpr;
    for (const s of link.spans) {
      const fg = grid.cellAt(s.x, s.y).style.fg;
      const u = document.createElement("div");
      u.className = "term-link-underline";
      Object.assign(u.style, {
        position: "absolute",
        left: `${(s.x * m.cellW) / m.dpr}px`,
        top: `${((s.y + 1) * m.cellH) / m.dpr - 2 * thick}px`,
        width: `${(s.len * m.cellW) / m.dpr}px`,
        height: `${thick}px`,
        background: css(fg),
      });
      this.el.append(u);
    }
    Object.assign(this.preview.style, {
      display: "block",
      color: css(grid.defaultFg),
      background: css(grid.defaultBg),
      border: `1px solid ${css(grid.defaultFg)}40`,
    });
    this.preview.textContent = link.target;
  }

  get link(): Link | null {
    return this.shown;
  }
}
