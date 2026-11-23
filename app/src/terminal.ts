import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/700.css";
import "@fontsource/jetbrains-mono/400-italic.css";
import "@fontsource/jetbrains-mono/700-italic.css";
import "./terminal.css";

import type { Cursor, Find, Frame, InputEvent } from "./proto";
import { CursorLayer } from "./cursor";
import { Grid } from "./grid";
import { attachCopy, attachPaste, copyNow } from "./input/clipboard";
import { ImeInput } from "./input/ime";
import { KeyboardInput, detectPlatform, type Platform } from "./input/keyboard";
import { MouseInput } from "./input/mouse";
import { LinkLayer } from "./links";
import { FindLayer } from "./findlayer";
import { ScrollbarLayer } from "./scrollbar";
import {
  DEFAULT_FONT_SIZE,
  computeMetrics,
  deviceRatio,
  loadFonts,
  measureFont,
  watchDpr,
  type Metrics,
} from "./metrics";
import { css } from "./painter/color";
import { Painter } from "./painter/painter";
import { gridSize, watchSize } from "./resize";
import type { Transport } from "./transport/types";

export interface PaintInfo {
  ms: number;
  /** Rows repainted. */
  rows: number;
  full: boolean;
  /** The repainted rows of a partial paint; null after a full repaint. */
  dirty: number[] | null;
  seq: number;
}

export interface TerminalOptions {
  fontSize?: number;
  /** Force a device pixel ratio instead of the window's (tests, harness). */
  dpr?: number;
  platform?: Platform;
  autofocus?: boolean;
  /** Called after each paint with how long painting took. */
  onPaint?: (info: PaintInfo) => void;
  /** Right-click menu; see `MouseInput`. Without it, right-click does nothing. */
  onContextMenu?: (e: MouseEvent) => void;
  /** The input gained or lost keyboard focus. */
  onFocusChange?: (focused: boolean) => void;
  /** The title changed (see `Grid.title`). */
  onTitle?: (title: string) => void;
  /** The search's results changed, or it ended (`null`). */
  onFind?: (find: Find | null) => void;
  /** Cmd+click (Ctrl+click) on a link: open `target`, a URL or an absolute path. */
  onOpenLink?: (target: string) => void;
}

function sameCursor(a: Cursor, b: Cursor): boolean {
  return (
    a.x === b.x &&
    a.y === b.y &&
    a.in_viewport === b.in_viewport &&
    a.visible === b.visible &&
    a.style === b.style &&
    a.wide === b.wide &&
    a.color === b.color
  );
}

/** How long a pane waits for the full frame it asked for before reattaching. */
const STALL_TIMEOUT_MS = 2000;

/** One terminal pane: canvases, input, and the frame loop. */
export class TerminalView {
  readonly root: HTMLDivElement;
  readonly gridEl: HTMLDivElement;
  readonly gridCanvas: HTMLCanvasElement;
  readonly overlay: HTMLCanvasElement;
  readonly textarea: HTMLTextAreaElement;
  /** Shows where the view is while scrolled back into history; drag it to scroll. */
  readonly scrollbar: ScrollbarLayer;
  /** Dims the pane when another pane is active. */
  readonly dimEl: HTMLDivElement;
  readonly grid = new Grid();
  readonly painter: Painter;
  readonly cursor: CursorLayer;
  readonly links: LinkLayer;
  readonly finds: FindLayer;
  m: Metrics;

  private pending: Frame[] = [];
  private raf = 0;
  private sentSize = "";
  /** When the last frame arrived, and the check for a stall under way. */
  private lastFrameAt = performance.now();
  private stallTimer = 0;
  private fullTimer = 0;
  private shown = true;
  /** Frames were applied while hidden, and not painted. */
  private stale = false;
  private focused = false;
  private fontSize: number;
  private readonly cleanups: Array<() => void> = [];

  static async create(container: HTMLElement, transport: Transport, opts: TerminalOptions = {}): Promise<TerminalView> {
    const fontSize = opts.fontSize ?? DEFAULT_FONT_SIZE;
    await loadFonts(fontSize);
    const view = new TerminalView(container, transport, opts, fontSize);
    view.layout(container.clientWidth, container.clientHeight);
    await transport.attach((f) => view.enqueue(f));
    if (opts.autofocus !== false) view.focus();
    return view;
  }

  private constructor(
    readonly container: HTMLElement,
    readonly transport: Transport,
    readonly opts: TerminalOptions,
    fontSize: number,
  ) {
    this.fontSize = fontSize;
    this.m = this.measure();

    this.root = document.createElement("div");
    this.root.className = "term";
    this.root.style.background = css(this.grid.defaultBg);

    this.gridEl = document.createElement("div");
    this.gridEl.className = "term-grid";

    this.gridCanvas = document.createElement("canvas");
    this.gridCanvas.className = "term-canvas";
    this.overlay = document.createElement("canvas");
    this.overlay.className = "term-overlay";

    this.textarea = document.createElement("textarea");
    this.textarea.className = "term-input";
    this.textarea.setAttribute("aria-label", "Terminal input");
    for (const [k, v] of Object.entries({ autocapitalize: "off", autocomplete: "off", autocorrect: "off", spellcheck: "false" })) {
      this.textarea.setAttribute(k, v);
    }

    this.dimEl = document.createElement("div");
    this.dimEl.className = "term-dim";
    this.dimEl.style.display = "none";

    this.gridEl.append(this.gridCanvas, this.overlay, this.textarea);
    this.root.append(this.gridEl);
    this.scrollbar = new ScrollbarLayer(this.root, {
      send: (e) => this.transport.input(e),
      wheel: (e) => {
        e.preventDefault();
        this.overlay.dispatchEvent(new WheelEvent("wheel", e));
      },
    });
    this.root.append(this.dimEl);
    container.append(this.root);

    this.painter = new Painter(this.gridCanvas, this.m);
    this.cursor = new CursorLayer(this.overlay, this.m);
    this.finds = new FindLayer(this.gridEl);
    this.links = new LinkLayer(this.gridEl, this.root);

    const send = (e: InputEvent) => this.transport.input(e);
    // Typing into a pane that has shown nothing for a while: check that
    // frames still come, so a stalled pane recovers when someone uses it.
    const sendTyped = (e: InputEvent) => {
      if (performance.now() - this.lastFrameAt > STALL_TIMEOUT_MS) this.checkFrames();
      send(e);
    };
    const platform = opts.platform ?? detectPlatform();
    const kb = new KeyboardInput(this.textarea, { send: sendTyped, activity: () => this.cursor.resetBlink(), copy: copyNow }, platform);
    const ime = new ImeInput(this.textarea, { send: sendTyped, preedit: (t) => this.cursor.setPreedit(t) });
    const mouse = new MouseInput(
      this.overlay,
      () => ({
        tracking: this.grid.mouseTracking,
        m: this.m,
        cols: this.painter.cols,
        rows: this.painter.rows,
        link: this.links.link,
      }),
      { send, focus: () => this.focus(), contextMenu: opts.onContextMenu, openLink: opts.onOpenLink },
      platform,
    );
    mouse.setTracking("none");
    this.mouse = mouse;
    this.cleanups.push(
      () => kb.dispose(),
      () => ime.dispose(),
      () => mouse.dispose(),
      () => this.scrollbar.dispose(),
      attachPaste(this.textarea, sendTyped),
      attachCopy(this.textarea, () => this.grid.selectionText),
      watchSize(container, (w, h) => this.layout(w, h)),
    );
    if (!opts.dpr) this.cleanups.push(watchDpr(() => this.remeasure()));

    const onFocus = () => this.setFocused(true);
    const onBlur = () => this.setFocused(false);
    this.textarea.addEventListener("focus", onFocus);
    this.textarea.addEventListener("blur", onBlur);
    const onRootDown = (e: MouseEvent) => {
      e.preventDefault(); // no selection, and focus stays on the textarea
      this.focus();
    };
    this.root.addEventListener("mousedown", onRootDown);
    this.cleanups.push(() => {
      this.textarea.removeEventListener("focus", onFocus);
      this.textarea.removeEventListener("blur", onBlur);
      this.root.removeEventListener("mousedown", onRootDown);
    });
    this.cleanups.push(this.watchLostPixels());
  }

  /**
   * Browsers may throw a canvas's pixels away (memory pressure, a graphics
   * reset, a window hidden a long time), and nothing else repaints until
   * the screen changes: a pane left alone went blank until scrolled. The
   * grid mirror still has everything, so paint it again when the canvas
   * comes back, and whenever the page is shown or the window focused, for
   * engines that clear it without saying so.
   */
  private watchLostPixels(): () => void {
    const onLost = () => console.warn("kibann: canvas lost its contents");
    const onRestored = () => this.repaint();
    // Back from sleep or from being hidden is also when frames are most
    // likely to have stopped: check that they still come.
    const onVisible = () => {
      if (document.visibilityState === "visible") {
        this.repaint();
        this.checkFrames();
      }
    };
    const onFocus = () => {
      this.repaint();
      this.checkFrames();
    };
    for (const c of [this.gridCanvas, this.overlay]) {
      c.addEventListener("contextlost", onLost);
      c.addEventListener("contextrestored", onRestored);
    }
    document.addEventListener("visibilitychange", onVisible);
    window.addEventListener("focus", onFocus);
    return () => {
      for (const c of [this.gridCanvas, this.overlay]) {
        c.removeEventListener("contextlost", onLost);
        c.removeEventListener("contextrestored", onRestored);
      }
      document.removeEventListener("visibilitychange", onVisible);
      window.removeEventListener("focus", onFocus);
    };
  }

  /** Paint everything again from the grid mirror (nothing changed in it). */
  repaint(): void {
    if (this.painter.cols <= 0) return; // not laid out yet
    this.painter.paintAll(this.grid);
    this.cursor.update(this.grid, false);
  }

  private readonly mouse: MouseInput;

  /** The selected text, or "" when nothing is selected. */
  get selectionText(): string {
    return this.grid.selectionText;
  }

  private measure(): Metrics {
    return computeMetrics(measureFont(this.fontSize), this.fontSize, deviceRatio(this.opts.dpr));
  }

  focus(): void {
    this.textarea.focus({ preventScroll: true });
  }

  private setFocused(focused: boolean): void {
    if (this.focused === focused) return;
    this.focused = focused;
    this.cursor.setFocused(focused);
    this.transport.input({ kind: "focus", focused });
    this.opts.onFocusChange?.(focused);
  }

  /** Whether this is the pane the user is working in; others are dimmed. */
  setActive(active: boolean): void {
    this.dimEl.style.display = active ? "none" : "block";
  }

  /** Use `size` (CSS px) and the current font families: re-measure and
   * repaint; the grid gets as many cells as now fit. */
  async setFont(size: number): Promise<void> {
    this.fontSize = size;
    await loadFonts(size);
    this.remeasure();
    this.painter.paintAll(this.grid);
    this.cursor.update(this.grid, true);
  }

  /** Re-measure after a DPR change; counts as a resize. */
  remeasure(): void {
    this.m = this.measure();
    this.painter.setMetrics(this.m);
    this.cursor.setMetrics(this.m);
    this.links.update(this.grid, this.m, true);
    this.finds.update(this.grid, this.m, true);
    this.painter.cols = -1; // force the layout pass to resize the canvases
    this.layout(this.container.clientWidth, this.container.clientHeight);
  }

  /** Fit the grid to the container; repaint from the mirror and tell the backend. */
  layout(widthCss: number, heightCss: number): void {
    if (widthCss <= 0 || heightCss <= 0) return;
    const { cols, rows } = gridSize(widthCss, heightCss, this.m);
    const padCss = this.m.pad / this.m.dpr;
    this.gridEl.style.left = `${padCss}px`;
    this.gridEl.style.top = `${padCss}px`;
    if (cols !== this.painter.cols || rows !== this.painter.rows) {
      this.grid.resizeLocal(cols, rows);
      this.painter.resize(cols, rows);
      this.cursor.resize(cols, rows);
      this.painter.paintAll(this.grid);
      this.cursor.update(this.grid, false);
      this.positionTextarea();
    }
    const size = { cols, rows, cell_w: this.m.cellW, cell_h: this.m.cellH };
    const key = JSON.stringify(size);
    if (key !== this.sentSize) {
      this.sentSize = key;
      this.transport.resize(size);
    }
  }

  /**
   * Ask for a full frame; if none arrives in time, the connection to the
   * session is stuck (see {@link Transport.reattach}): make a new one.
   */
  checkFrames(): void {
    if (this.stallTimer || !this.transport.reattach) return;
    const asked = performance.now();
    this.transport.requestFull();
    this.stallTimer = window.setTimeout(() => {
      this.stallTimer = 0;
      if (this.lastFrameAt >= asked) return;
      console.warn("kibann: frames stopped coming; reattaching");
      void this.transport.reattach?.();
    }, STALL_TIMEOUT_MS);
  }

  /** Ask for the full frame the grid waits for, and again until it comes:
   * with one lost on the way, a pane nobody types into (and its thumbnail)
   * would stay stale. */
  private askFull(): void {
    this.transport.requestFull();
    clearTimeout(this.fullTimer);
    this.fullTimer = window.setTimeout(() => {
      this.fullTimer = 0;
      if (this.grid.awaitingFull) this.askFull();
    }, STALL_TIMEOUT_MS);
  }

  enqueue(frame: Frame): void {
    this.lastFrameAt = performance.now();
    this.pending.push(frame);
    if (!this.raf) this.raf = requestAnimationFrame(() => this.flush());
  }

  /** Apply every frame that arrived since the last animation frame, then paint once. */
  flush(): void {
    this.raf = 0;
    const frames = this.pending;
    this.pending = [];
    const prevCursor = this.grid.cursor;
    const prevTitle = this.grid.title;
    const prevFind = this.grid.find;
    const dirty = new Set<number>();
    let full = false;
    let ackSeq: number | null = null;
    let askedFull = false;
    for (const f of frames) {
      const r = this.grid.apply(f);
      if (!r.applied) {
        if (r.requestFull && !askedFull) {
          askedFull = true;
          this.askFull();
        }
        continue;
      }
      ackSeq = f.seq;
      if (r.full) full = true;
      for (const y of r.dirty) dirty.add(y);
    }
    if (ackSeq === null) return;

    const t0 = performance.now();
    if (this.shown) {
      if (this.fitPainter()) full = true;
      if (full) this.painter.paintAll(this.grid);
      else this.painter.paintRows(this.grid, dirty);
      this.cursor.update(this.grid, !sameCursor(prevCursor, this.grid.cursor));
    } else this.stale = true;
    const ms = performance.now() - t0;

    this.root.style.background = css(this.grid.defaultBg);
    this.dimEl.style.background = css(this.grid.defaultBg);
    if (this.grid.title !== prevTitle) this.opts.onTitle?.(this.grid.title);
    this.mouse.setTracking(this.grid.mouseTracking);
    if (this.shown) this.decorate();
    if (this.grid.find !== prevFind) this.opts.onFind?.(this.grid.find);
    this.transport.ack(ackSeq);
    this.opts.onPaint?.({
      ms,
      rows: full ? this.grid.rows : dirty.size,
      full,
      dirty: full ? null : [...dirty].sort((a, b) => a - b),
      seq: ackSeq,
    });
  }

  /** The backend's size wins; a mismatch settles once our resize reaches
   * it. Whether the painter had to change size (and so paint everything). */
  private fitPainter(): boolean {
    if (this.grid.cols === this.painter.cols && this.grid.rows === this.painter.rows) return false;
    this.painter.resize(this.grid.cols, this.grid.rows);
    this.cursor.resize(this.grid.cols, this.grid.rows);
    return true;
  }

  /** What sits over the grid: the scrollbar, links, find matches and the
   * input's position. */
  private decorate(): void {
    this.updateScrollbar();
    this.links.update(this.grid, this.m);
    this.finds.update(this.grid, this.m);
    this.mouse.setLinkShown(this.links.link !== null);
    this.positionTextarea();
  }

  /** Whether the pane is on screen. A hidden one keeps its grid up to date
   * but paints only once it's shown, or when its picture is asked for. */
  setShown(shown: boolean): void {
    if (this.shown === shown) return;
    this.shown = shown;
    if (shown) this.catchUp();
  }

  /** The grid canvas, painted up to the latest frame (for thumbnails). */
  picture(): HTMLCanvasElement {
    this.catchUp();
    return this.gridCanvas;
  }

  /** Paint what frames changed while the pane was hidden. */
  private catchUp(): void {
    if (!this.stale) return;
    this.stale = false;
    this.fitPainter();
    this.repaint();
    if (this.shown) this.decorate();
  }

  private updateScrollbar(): void {
    this.scrollbar.update(this.grid.scrollbar, this.m.pad / this.m.dpr, this.root.clientHeight, css(this.grid.defaultFg));
  }

  private positionTextarea(): void {
    const p = this.cursor.cursorCss(this.grid);
    Object.assign(this.textarea.style, {
      left: `${p.left}px`,
      top: `${p.top}px`,
      height: `${p.height}px`,
      fontSize: `${this.fontSize}px`,
      lineHeight: `${p.height}px`,
    });
  }

  dispose(): void {
    clearTimeout(this.stallTimer);
    clearTimeout(this.fullTimer);
    if (this.raf) cancelAnimationFrame(this.raf);
    this.cursor.dispose();
    for (const c of this.cleanups) c();
    this.root.remove();
  }
}
