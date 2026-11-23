import type { InputEvent, Scrollbar } from "./proto";
import "./terminal.css";

/** Thumb width at rest and while hovered or dragged, in CSS px. */
const THIN = 4;
const WIDE = 8;
const MIN_THUMB = 16;

export interface ScrollbarHooks {
  send: (e: InputEvent) => void;
  /** Wheel over the bar scrolls like wheel over the grid. */
  wheel: (e: WheelEvent) => void;
}

interface Drag {
  pointer: number;
  startY: number;
  startOffset: number;
  /** Where the drag has asked the view to be; scroll events are deltas from it. */
  sent: number;
}

/**
 * The scrollback bar: shown only while scrolled back. Drag the thumb to
 * scroll, click the track above or below it to page. Thin at rest, wider
 * under the pointer.
 */
export class ScrollbarLayer {
  /** The strip that takes the pointer. */
  readonly track: HTMLDivElement;
  readonly thumb: HTMLDivElement;

  private sb: Scrollbar = { total: 0, offset: 0, len: 0 };
  private color = "";
  private pad = 0;
  private height = 0;
  private hovered = false;
  private drag: Drag | null = null;
  private readonly offs: Array<() => void> = [];

  constructor(parent: HTMLElement, private readonly hooks: ScrollbarHooks) {
    this.track = document.createElement("div");
    this.track.className = "term-scrollbar-track";
    this.track.style.display = "none";
    this.thumb = document.createElement("div");
    this.thumb.className = "term-scrollbar";
    this.track.append(this.thumb);
    parent.append(this.track);

    const on = <K extends keyof HTMLElementEventMap>(type: K, fn: (e: HTMLElementEventMap[K]) => void, opts?: AddEventListenerOptions) => {
      this.track.addEventListener(type, fn, opts);
      this.offs.push(() => this.track.removeEventListener(type, fn, opts));
    };
    on("pointerenter", () => this.setHovered(true));
    on("pointerleave", () => this.setHovered(false));
    on("pointerdown", (e) => this.onDown(e));
    on("pointermove", (e) => this.onMove(e));
    on("pointerup", (e) => this.onUp(e));
    on("pointercancel", (e) => this.onUp(e));
    on("lostpointercapture", () => this.endDrag());
    on("wheel", (e) => this.hooks.wheel(e), { passive: false });
    on("contextmenu", (e) => e.preventDefault());
  }

  /** A new position from the backend, the pane's padding and height in CSS px, and the thumb color. */
  update(sb: Scrollbar, pad: number, height: number, color: string): void {
    this.sb = sb;
    this.pad = pad;
    this.height = height;
    this.color = color;
    // At the bottom the backend's offset follows new output: track it so a
    // drag back up starts from where the view really is.
    if (this.drag && sb.offset + sb.len >= sb.total) this.drag.sent = Math.max(0, sb.total - sb.len);
    this.render();
  }

  get visible(): boolean {
    return this.track.style.display !== "none";
  }

  private get maxOffset(): number {
    return Math.max(0, this.sb.total - this.sb.len);
  }

  private geometry(): { track: number; thumb: number } {
    const track = Math.max(0, this.height - 2 * this.pad);
    const { total, len } = this.sb;
    const thumb = Math.min(track, Math.max(MIN_THUMB, (len / Math.max(1, total)) * track));
    return { track, thumb };
  }

  private render(): void {
    const { total, offset, len } = this.sb;
    const scrolledBack = total > len && offset + len < total;
    if (!scrolledBack && !this.drag) {
      this.track.style.display = "none";
      return;
    }
    const g = this.geometry();
    // While dragging, follow the pointer rather than the round trip.
    const at = this.drag ? this.drag.sent : offset;
    const top = (at / Math.max(1, this.maxOffset)) * (g.track - g.thumb);
    const wide = this.hovered || this.drag !== null;
    const w = wide ? WIDE : THIN;
    Object.assign(this.track.style, { display: "block", top: `${this.pad}px`, height: `${g.track}px` });
    Object.assign(this.thumb.style, {
      top: `${top}px`,
      height: `${g.thumb}px`,
      width: `${w}px`,
      borderRadius: `${w / 2}px`,
      background: this.color,
      opacity: this.drag ? "0.6" : wide ? "0.5" : "0.35",
    });
  }

  private setHovered(on: boolean): void {
    if (this.hovered === on) return;
    this.hovered = on;
    this.render();
  }

  private onDown(e: PointerEvent): void {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    const r = this.thumb.getBoundingClientRect();
    if (e.clientY < r.top) {
      this.hooks.send({ kind: "scroll", by: "pages", n: -1 });
      return;
    }
    if (e.clientY > r.bottom) {
      this.hooks.send({ kind: "scroll", by: "pages", n: 1 });
      return;
    }
    this.track.setPointerCapture(e.pointerId);
    this.drag = { pointer: e.pointerId, startY: e.clientY, startOffset: this.sb.offset, sent: this.sb.offset };
    this.render();
  }

  private onMove(e: PointerEvent): void {
    const d = this.drag;
    if (!d || e.pointerId !== d.pointer) return;
    const g = this.geometry();
    const free = g.track - g.thumb;
    if (free <= 0) return;
    const rowsPerPx = this.maxOffset / free;
    const target = Math.round(Math.min(this.maxOffset, Math.max(0, d.startOffset + (e.clientY - d.startY) * rowsPerPx)));
    if (target === d.sent) return;
    this.hooks.send({ kind: "scroll", by: "rows", n: target - d.sent });
    d.sent = target;
    this.render();
  }

  private onUp(e: PointerEvent): void {
    if (this.drag && e.pointerId === this.drag.pointer && this.track.hasPointerCapture(e.pointerId)) {
      this.track.releasePointerCapture(e.pointerId);
    }
    this.endDrag();
  }

  private endDrag(): void {
    if (!this.drag) return;
    this.drag = null;
    this.render();
  }

  dispose(): void {
    for (const off of this.offs) off();
    this.track.remove();
  }
}
