import type { InputEvent, MouseTracking } from "../proto";
import type { Metrics } from "../metrics";
import { modsOf } from "./keyboard";

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
}

/** Pointer and wheel events on the grid, sent in device px from the grid origin. */
export class MouseInput {
  private wheel = new WheelAccumulator();
  private lastCell = -1;
  private readonly offs: Array<() => void> = [];

  constructor(
    readonly target: HTMLElement,
    private readonly state: () => MouseState,
    private readonly send: (e: InputEvent) => void,
    private readonly focus: () => void,
  ) {
    const on = <K extends keyof HTMLElementEventMap>(type: K, fn: (e: HTMLElementEventMap[K]) => void, opts?: AddEventListenerOptions) => {
      target.addEventListener(type, fn, opts);
      this.offs.push(() => target.removeEventListener(type, fn, opts));
    };
    on("pointerdown", (e) => this.onDown(e));
    on("pointerup", (e) => this.onUp(e));
    on("pointermove", (e) => this.onMove(e));
    on("wheel", (e) => this.onWheel(e), { passive: false });
    on("contextmenu", (e) => e.preventDefault());
    on("mousedown", (e) => e.preventDefault()); // keep focus on the textarea
  }

  setTracking(t: MouseTracking): void {
    this.target.style.cursor = t === "none" ? "text" : "default";
  }

  private pos(e: MouseEvent): { x: number; y: number } {
    const { m, cols, rows } = this.state();
    const rect = this.target.getBoundingClientRect();
    const x = Math.min(Math.max(0, (e.clientX - rect.left) * m.dpr), cols * m.cellW - 1);
    const y = Math.min(Math.max(0, (e.clientY - rect.top) * m.dpr), rows * m.cellH - 1);
    return { x, y };
  }

  private onDown(e: PointerEvent): void {
    this.focus();
    const button = buttonOf(e.button);
    if (button === null) return;
    try {
      this.target.setPointerCapture(e.pointerId);
    } catch {
      /* synthetic events have no capturable pointer */
    }
    const { x, y } = this.pos(e);
    this.send({ kind: "mouse", action: "press", button, x, y, mods: modsOf(e) });
  }

  private onUp(e: PointerEvent): void {
    const button = buttonOf(e.button);
    if (button === null) return;
    const { x, y } = this.pos(e);
    this.send({ kind: "mouse", action: "release", button, x, y, mods: modsOf(e) });
  }

  private onMove(e: PointerEvent): void {
    const { tracking, m, cols } = this.state();
    if (tracking !== "any" && !(tracking === "button" && e.buttons !== 0)) return;
    const { x, y } = this.pos(e);
    const cell = Math.floor(y / m.cellH) * cols + Math.floor(x / m.cellW);
    if (cell === this.lastCell) return;
    this.lastCell = cell;
    const held = e.buttons & 1 ? 1 : e.buttons & 4 ? 3 : e.buttons & 2 ? 2 : null;
    this.send({ kind: "mouse", action: "motion", button: held, x, y, mods: modsOf(e) });
  }

  private onWheel(e: WheelEvent): void {
    e.preventDefault();
    const { m, rows } = this.state();
    const { dx, dy } = this.wheel.push(e.deltaX, e.deltaY, e.deltaMode, m.cellW / m.dpr, m.cellH / m.dpr, rows);
    if (!dx && !dy) return;
    const { x, y } = this.pos(e);
    this.send({ kind: "wheel", dx_rows: dx, dy_rows: dy, x, y, mods: modsOf(e) });
  }

  dispose(): void {
    for (const off of this.offs) off();
  }
}
