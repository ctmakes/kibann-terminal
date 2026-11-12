import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/700.css";
import "@fontsource/jetbrains-mono/400-italic.css";
import "@fontsource/jetbrains-mono/700-italic.css";

import type { Cursor, Frame, InputEvent } from "./proto";
import { CursorLayer } from "./cursor";
import { Grid } from "./grid";
import { attachCopy, attachPaste, copyNow } from "./input/clipboard";
import { ImeInput } from "./input/ime";
import { KeyboardInput, detectPlatform, type Platform } from "./input/keyboard";
import { MouseInput } from "./input/mouse";
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

/** One terminal pane: canvases, input, and the frame loop. */
export class TerminalView {
  readonly root: HTMLDivElement;
  readonly gridEl: HTMLDivElement;
  readonly gridCanvas: HTMLCanvasElement;
  readonly overlay: HTMLCanvasElement;
  readonly textarea: HTMLTextAreaElement;
  /** Shows where the view is while scrolled back into history. */
  readonly scrollbarEl: HTMLDivElement;
  readonly grid = new Grid();
  readonly painter: Painter;
  readonly cursor: CursorLayer;
  m: Metrics;

  private pending: Frame[] = [];
  private raf = 0;
  private sentSize = "";
  private focused = false;
  private readonly fontSize: number;
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
    Object.assign(this.root.style, {
      position: "relative",
      width: "100%",
      height: "100%",
      overflow: "hidden",
      background: css(this.grid.defaultBg),
      // Nothing here is selectable page content; without this a click in the
      // padding starts a WebKit selection that tints the whole canvas blue.
      userSelect: "none",
      webkitUserSelect: "none",
    });

    this.gridEl = document.createElement("div");
    this.gridEl.className = "term-grid";
    Object.assign(this.gridEl.style, { position: "absolute" });

    this.gridCanvas = document.createElement("canvas");
    this.gridCanvas.className = "term-canvas";
    this.overlay = document.createElement("canvas");
    this.overlay.className = "term-overlay";
    for (const c of [this.gridCanvas, this.overlay]) {
      Object.assign(c.style, { position: "absolute", left: "0", top: "0", display: "block" });
    }

    this.textarea = document.createElement("textarea");
    this.textarea.className = "term-input";
    this.textarea.setAttribute("aria-label", "Terminal input");
    for (const [k, v] of Object.entries({ autocapitalize: "off", autocomplete: "off", autocorrect: "off", spellcheck: "false" })) {
      this.textarea.setAttribute(k, v);
    }
    Object.assign(this.textarea.style, {
      position: "absolute",
      opacity: "0",
      width: "2px",
      padding: "0",
      border: "0",
      margin: "0",
      outline: "none",
      resize: "none",
      overflow: "hidden",
      whiteSpace: "nowrap",
      caretColor: "transparent",
      // The root disables selection; WebKit can then refuse typing into a
      // textarea that inherits it.
      userSelect: "text",
      webkitUserSelect: "text",
      color: "transparent",
      background: "transparent",
      zIndex: "-1",
    });

    this.scrollbarEl = document.createElement("div");
    this.scrollbarEl.className = "term-scrollbar";
    Object.assign(this.scrollbarEl.style, {
      position: "absolute",
      right: "2px",
      width: "4px",
      borderRadius: "2px",
      pointerEvents: "none",
      display: "none",
    });

    this.gridEl.append(this.gridCanvas, this.overlay, this.textarea);
    this.root.append(this.gridEl, this.scrollbarEl);
    container.append(this.root);

    this.painter = new Painter(this.gridCanvas, this.m);
    this.cursor = new CursorLayer(this.overlay, this.m);

    const send = (e: InputEvent) => this.transport.input(e);
    const platform = opts.platform ?? detectPlatform();
    const kb = new KeyboardInput(this.textarea, { send, activity: () => this.cursor.resetBlink(), copy: copyNow }, platform);
    const ime = new ImeInput(this.textarea, { send, preedit: (t) => this.cursor.setPreedit(t) });
    const mouse = new MouseInput(
      this.overlay,
      () => ({ tracking: this.grid.mouseTracking, m: this.m, cols: this.painter.cols, rows: this.painter.rows }),
      send,
      () => this.focus(),
      opts.onContextMenu,
    );
    mouse.setTracking("none");
    this.mouse = mouse;
    this.cleanups.push(
      () => kb.dispose(),
      () => ime.dispose(),
      () => mouse.dispose(),
      attachPaste(this.textarea, send),
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
  }

  /** Re-measure after a DPR change; counts as a resize. */
  remeasure(): void {
    this.m = this.measure();
    this.painter.setMetrics(this.m);
    this.cursor.setMetrics(this.m);
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

  enqueue(frame: Frame): void {
    this.pending.push(frame);
    if (!this.raf) this.raf = requestAnimationFrame(() => this.flush());
  }

  /** Apply every frame that arrived since the last animation frame, then paint once. */
  flush(): void {
    this.raf = 0;
    const frames = this.pending;
    this.pending = [];
    const prevCursor = this.grid.cursor;
    const dirty = new Set<number>();
    let full = false;
    let ackSeq: number | null = null;
    let askedFull = false;
    for (const f of frames) {
      const r = this.grid.apply(f);
      if (!r.applied) {
        if (r.requestFull && !askedFull) {
          askedFull = true;
          this.transport.requestFull();
        }
        continue;
      }
      ackSeq = f.seq;
      if (r.full) full = true;
      for (const y of r.dirty) dirty.add(y);
    }
    if (ackSeq === null) return;

    // The backend's size wins; a mismatch settles once our resize reaches it.
    if (this.grid.cols !== this.painter.cols || this.grid.rows !== this.painter.rows) {
      this.painter.resize(this.grid.cols, this.grid.rows);
      this.cursor.resize(this.grid.cols, this.grid.rows);
      full = true;
    }

    const t0 = performance.now();
    if (full) this.painter.paintAll(this.grid);
    else this.painter.paintRows(this.grid, dirty);
    this.cursor.update(this.grid, !sameCursor(prevCursor, this.grid.cursor));
    const ms = performance.now() - t0;

    this.root.style.background = css(this.grid.defaultBg);
    this.updateScrollbar();
    this.mouse.setTracking(this.grid.mouseTracking);
    this.positionTextarea();
    this.transport.ack(ackSeq);
    this.opts.onPaint?.({
      ms,
      rows: full ? this.grid.rows : dirty.size,
      full,
      dirty: full ? null : [...dirty].sort((a, b) => a - b),
      seq: ackSeq,
    });
  }

  /** Visible only while scrolled back, sized and placed like the viewport in the history. */
  private updateScrollbar(): void {
    const { total, offset, len } = this.grid.scrollbar;
    const el = this.scrollbarEl;
    if (total <= len || offset + len >= total) {
      el.style.display = "none";
      return;
    }
    const pad = this.m.pad / this.m.dpr;
    const track = this.root.clientHeight - 2 * pad;
    const h = Math.max(16, (len / total) * track);
    const top = pad + (offset / Math.max(1, total - len)) * (track - h);
    Object.assign(el.style, {
      display: "block",
      top: `${top}px`,
      height: `${h}px`,
      background: css(this.grid.defaultFg),
      opacity: "0.35",
    });
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
    if (this.raf) cancelAnimationFrame(this.raf);
    this.cursor.dispose();
    for (const c of this.cleanups) c();
    this.root.remove();
  }
}
