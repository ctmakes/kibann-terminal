import type { InputEvent, Link, MouseTracking } from "../proto";
import type { Metrics } from "../metrics";
import { linkCovers } from "../links";
import { modsOf, type Platform } from "./keyboard";

/** The modifier that turns hovering and clicking into link actions: Cmd on
 * macOS, Ctrl elsewhere (Ghostty's defaults). */
export function linkModifier(e: { metaKey: boolean; ctrlKey: boolean }, platform: Platform): boolean {
  return platform === "mac" ? e.metaKey : e.ctrlKey;
}

/** DOM `MouseEvent.button` → libghostty button number. */
export function buttonOf(domButton: number): number | null {
  switch (domButton) {
    case 0: return 1;
    case 1: return 3;
    case 2: return 2;
    case 3: return 8;
    case 4: return 9;
    default: return null;
  }
}

/** Turns wheel deltas into whole rows, carrying the remainder between events. */
export class WheelAccumulator {
  private x = 0;
  private y = 0;

  /** `cellW`/`cellH` in CSS px. Returns whole rows/columns to send now. */
  push(dx: number, dy: number, mode: number, cellW: number, cellH: number, rows: number): { dx: number; dy: number } {
    if (mode === 1) {
      this.x += dx;
      this.y += dy;
    } else if (mode === 2) {
      this.x += dx * rows;
      this.y += dy * rows;
    } else {
      this.x += dx / cellW;
      this.y += dy / cellH;
    }
    const ox = Math.trunc(this.x);
    const oy = Math.trunc(this.y);
    this.x -= ox;
    this.y -= oy;
    return { dx: ox, dy: oy };
  }

  reset(): void {
    this.x = this.y = 0;
  }
}

export interface MouseState {
  tracking: MouseTracking;
  m: Metrics;
  cols: number;
  rows: number;
  /** The link the backend says is under the pointer. */
  link: Link | null;
}

export interface MouseHooks {
  send: (e: InputEvent) => void;
  focus: () => void;
  /** Show the app's context menu. Called on right-click when no program is
   * tracking the mouse, or with Shift held (which keeps the click from the program). */
  contextMenu?: (e: MouseEvent) => void;
  /** Cmd+click (Ctrl+click) on a link. */
  openLink?: (target: string) => void;
}

/**
 * Pointer and wheel events on the grid, sent in device px from the grid origin.
 * Positions are not clamped: a selection drag past an edge autoscrolls, and
 * the backend clamps what it reports to programs.
 */
export class MouseInput {
  private wheel = new WheelAccumulator();
  private lastCell = -1;
  private readonly offs: Array<() => void> = [];
  /** Last pointer position over the grid (device px), or null when outside. */
  private at: { x: number; y: number } | null = null;
  /** A hover with the link modifier was sent and not yet turned off. */
  private hovering = false;
  private lastHoverCell = -1;
  /** A press opened a link; its release is ours too. */
  private swallowRelease = false;
  private tracking: MouseTracking = "none";
  private linkShown = false;

  constructor(
    readonly target: HTMLElement,
    private readonly state: () => MouseState,
    private readonly hooks: MouseHooks,
    private readonly platform: Platform = "mac",
  ) {
    const on = <K extends keyof HTMLElementEventMap>(type: K, fn: (e: HTMLElementEventMap[K]) => void, opts?: AddEventListenerOptions) => {
      target.addEventListener(type, fn, opts);
      this.offs.push(() => target.removeEventListener(type, fn, opts));
    };
    on("pointerdown", (e) => this.onDown(e));
    on("pointerup", (e) => this.onUp(e));
    on("pointermove", (e) => this.onMove(e));
    on("pointerleave", () => {
      this.at = null;
      this.hover(false);
    });
    on("wheel", (e) => this.onWheel(e), { passive: false });
    on("contextmenu", (e) => {
      e.preventDefault();
      if (this.hooks.contextMenu && (this.state().tracking === "none" || e.shiftKey)) this.hooks.contextMenu(e);
    });
    on("mousedown", (e) => e.preventDefault()); // keep focus on the textarea

    // Pressing or releasing the link modifier with the pointer still.
    const onKey = (e: KeyboardEvent) => {
      if (this.at) this.hover(linkModifier(e, this.platform));
    };
    const onBlur = () => this.hover(false);
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("keyup", onKey, true);
    window.addEventListener("blur", onBlur);
    this.offs.push(() => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("keyup", onKey, true);
      window.removeEventListener("blur", onBlur);
    });
  }

  setTracking(t: MouseTracking): void {
    this.tracking = t;
    this.updateCursor();
  }

  /** A link is shown under the pointer: point at it. */
  setLinkShown(shown: boolean): void {
    this.linkShown = shown;
    this.updateCursor();
  }

  private updateCursor(): void {
    this.target.style.cursor = this.linkShown ? "pointer" : this.tracking === "none" ? "text" : "default";
  }

  /** Tell the backend where the pointer is while the link modifier is held,
   * and once when it no longer is. */
  private hover(on: boolean): void {
    const at = this.at;
    if (on && at) {
      const { m, cols } = this.state();
      const cell = Math.floor(at.y / m.cellH) * cols + Math.floor(at.x / m.cellW);
      if (this.hovering && cell === this.lastHoverCell) return;
      this.hovering = true;
      this.lastHoverCell = cell;
      this.hooks.send({ kind: "hover", x: at.x, y: at.y, on: true });
    } else if (this.hovering) {
      this.hovering = false;
      this.lastHoverCell = -1;
      this.hooks.send({ kind: "hover", x: at?.x ?? 0, y: at?.y ?? 0, on: false });
    }
  }

  /** The shown link, if the pointer at `x`, `y` (device px) is on it. */
  private linkAt(x: number, y: number): Link | null {
    const { m, link } = this.state();
    if (!link) return null;
    return linkCovers(link, Math.floor(x / m.cellW), Math.floor(y / m.cellH)) ? link : null;
  }

  private pos(e: MouseEvent): { x: number; y: number } {
    const { m } = this.state();
    const rect = this.target.getBoundingClientRect();
    return { x: (e.clientX - rect.left) * m.dpr, y: (e.clientY - rect.top) * m.dpr };
  }

  /** Shift+right-click opens the menu instead of reaching the program. */
  private forMenu(e: PointerEvent): boolean {
    return !!this.hooks.contextMenu && e.button === 2 && e.shiftKey;
  }

  private onDown(e: PointerEvent): void {
    this.hooks.focus();
    const button = buttonOf(e.button);
    if (button === null || this.forMenu(e)) return;
    const { x, y } = this.pos(e);
    if (button === 1 && linkModifier(e, this.platform)) {
      const link = this.linkAt(x, y);
      if (link && this.hooks.openLink) {
        this.swallowRelease = true;
        this.hooks.openLink(link.target);
        return;
      }
    }
    try {
      this.target.setPointerCapture(e.pointerId);
    } catch {
      /* synthetic events have no capturable pointer */
    }
    this.hooks.send({ kind: "mouse", action: "press", button, x, y, mods: modsOf(e) });
  }

  private onUp(e: PointerEvent): void {
    const button = buttonOf(e.button);
    if (button === null || this.forMenu(e)) return;
    if (button === 1 && this.swallowRelease) {
      this.swallowRelease = false;
      return;
    }
    const { x, y } = this.pos(e);
    this.hooks.send({ kind: "mouse", action: "release", button, x, y, mods: modsOf(e) });
  }

  private onMove(e: PointerEvent): void {
    this.at = this.pos(e);
    this.hover(linkModifier(e, this.platform) && e.buttons === 0);
    const { tracking, m, cols } = this.state();
    // A drag the backend turns into a selection: every move counts, since
    // the selection's end depends on where in the cell the pointer is.
    const selecting = e.buttons & 1 && (tracking === "none" || e.shiftKey);
    if (!selecting) {
      if (tracking !== "any" && !(tracking === "button" && e.buttons !== 0)) return;
    }
    const { x, y } = this.pos(e);
    if (!selecting) {
      // Programs only see cells; skip moves within one.
      const cell = Math.floor(y / m.cellH) * cols + Math.floor(x / m.cellW);
      if (cell === this.lastCell) return;
      this.lastCell = cell;
    }
    const held = e.buttons & 1 ? 1 : e.buttons & 4 ? 3 : e.buttons & 2 ? 2 : null;
    this.hooks.send({ kind: "mouse", action: "motion", button: held, x, y, mods: modsOf(e) });
  }

  private onWheel(e: WheelEvent): void {
    e.preventDefault();
    const { m, rows } = this.state();
    const { dx, dy } = this.wheel.push(e.deltaX, e.deltaY, e.deltaMode, m.cellW / m.dpr, m.cellH / m.dpr, rows);
    if (!dx && !dy) return;
    const { x, y } = this.pos(e);
    this.hooks.send({ kind: "wheel", dx_rows: dx, dy_rows: dy, x, y, mods: modsOf(e) });
  }

  dispose(): void {
    for (const off of this.offs) off();
  }
}
