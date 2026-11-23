// Where a tab's panes go in the stage, and the dividers between them.
// Panes are absolutely positioned from `rects`, so a resize is one pass;
// dividers are plain elements on top.

import { dividers, ratioAt, rects, type Path, type PaneId, type Rect, type Tree } from "./layout";

/** The grab area across a divider, in CSS px. */
const GRAB = 6;

/** Each pane's box in `box`: the zoomed pane alone, or with `solo` the
 * focused one, else the split's. Panes left out aren't shown. */
export function paneRects(
  tab: { tree: Tree; zoomed: PaneId | null; focused: PaneId },
  box: Rect,
  solo: boolean,
): Map<PaneId, Rect> {
  if (tab.zoomed !== null) return new Map([[tab.zoomed, box]]);
  if (solo) return new Map([[tab.focused, box]]);
  return rects(tab.tree, box);
}

/** Put each element at its box, and hide those without one. */
export function place(els: Iterable<[PaneId, HTMLElement]>, at: Map<PaneId, Rect>): void {
  for (const [id, el] of els) {
    const r = at.get(id);
    if (!r) {
      el.style.display = "none";
      continue;
    }
    Object.assign(el.style, { display: "block", left: `${r.x}px`, top: `${r.y}px`, width: `${r.w}px`, height: `${r.h}px` });
  }
}

/** Where a divider's grab area goes. */
function grabArea(d: { dir: string; rect: Rect }): Partial<CSSStyleDeclaration> {
  const row = d.dir === "row";
  return {
    left: `${row ? d.rect.x - GRAB / 2 : d.rect.x}px`,
    top: `${row ? d.rect.y : d.rect.y - GRAB / 2}px`,
    width: `${row ? d.rect.w + GRAB : d.rect.w}px`,
    height: `${row ? d.rect.h : d.rect.h + GRAB}px`,
  };
}

/** Replace the dividers in `into` with those of `tree()` in `box()`.
 * Dragging one calls `drag` with its split and the new ratio, as often as
 * the pointer moves; letting go calls `drop` (with null if it didn't
 * move). `origin` is where the box is on the page. */
export function makeDividers(
  into: HTMLElement,
  tree: () => Tree,
  box: () => Rect,
  origin: () => DOMRect,
  on: { drag(path: Path, ratio: number): void; drop(path: Path, ratio: number | null): void },
): void {
  for (const d of into.querySelectorAll(".divider")) d.remove();
  for (const d of dividers(tree(), box())) {
    const el = document.createElement("div");
    el.className = "divider";
    Object.assign(el.style, grabArea(d), { cursor: d.dir === "row" ? "col-resize" : "row-resize" });
    el.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      el.setPointerCapture(e.pointerId);
      const at = origin();
      let ratio: number | null = null;
      const move = (ev: PointerEvent) => {
        ratio = ratioAt(d, ev.clientX - at.left, ev.clientY - at.top);
        on.drag(d.path, ratio);
        // Keep the grab area under the pointer while dragging.
        const next = dividers(tree(), box()).find((n) => n.path.join() === d.path.join());
        if (next) {
          const { left, top } = grabArea(next);
          Object.assign(el.style, { left, top });
        }
      };
      const up = () => {
        el.removeEventListener("pointermove", move);
        el.removeEventListener("pointerup", up);
        on.drop(d.path, ratio);
      };
      el.addEventListener("pointermove", move);
      el.addEventListener("pointerup", up);
    });
    into.append(el);
  }
}
