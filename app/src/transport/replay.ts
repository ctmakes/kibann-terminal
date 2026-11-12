import type { Frame, InputEvent, Link, Resize } from "../proto";
import { linkCovers } from "../links";
import { buildFrame, Screen } from "../dev/screen";
import type { Transport } from "./types";

/** Produces the screen for a fixture at a size and animation tick. */
export interface ScreenSource {
  build(cols: number, rows: number, tick: number): Screen;
  /** Milliseconds between ticks; omit for a still fixture. */
  interval?: number;
}

/**
 * Plays a fixture as if it were the backend: full frame on attach, resize or
 * request; partial frames on each tick, held back while two are unacked.
 */
export class ReplayTransport implements Transport {
  readonly inputs: InputEvent[] = [];
  readonly resizes: Resize[] = [];
  onInput?: (e: InputEvent) => void;
  framesSent = 0;
  fullRequests = 0;
  /** Test hook: skip sending the next frame, as if it were lost. */
  dropNext = false;
  onExit?: (code: number | null) => void;
  closed = false;
  /** Sent with the next frame when it changes. */
  title = "";
  private sentTitle = "";
  /** Test hook: links on the screen. A hover over one sends it, as the
   * backend would. */
  links: Link[] = [];
  private sentLink: Link | null = null;
  private onFrame: ((f: Frame) => void) | null = null;
  private seq = 0;
  private acked = 0;
  private tick = 0;
  private size: Resize = { cols: 80, rows: 24, cell_w: 8, cell_h: 16 };
  private last: Screen | null = null;
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(private readonly source: ScreenSource) {}

  async attach(onFrame: (f: Frame) => void): Promise<void> {
    this.onFrame = onFrame;
    this.sendFull();
    if (this.source.interval) {
      this.timer = setInterval(() => this.step(), this.source.interval);
    }
  }

  /** Advance one tick and send a partial frame, unless the frontend is behind. */
  step(): boolean {
    if (this.seq - this.acked > 1) return false;
    this.tick++;
    this.send(false);
    return true;
  }

  input(e: InputEvent): void {
    this.inputs.push(e);
    this.onInput?.(e);
    if (e.kind === "hover") {
      const x = Math.floor(e.x / this.size.cell_w);
      const y = Math.floor(e.y / this.size.cell_h);
      const link = (e.on && this.links.find((l) => linkCovers(l, x, y))) || null;
      if (link !== this.sentLink) {
        this.sentLink = link;
        this.send(false, link ?? { uri: "", target: "", spans: [] });
      }
    }
  }

  resize(size: Resize): void {
    this.resizes.push(size);
    this.size = size;
    if (this.onFrame) this.sendFull();
  }

  ack(seq: number): void {
    this.acked = Math.max(this.acked, seq);
  }

  requestFull(): void {
    this.fullRequests++;
    this.sendFull();
  }

  private sendFull(): void {
    this.send(true);
  }

  private send(full: boolean, link?: Link): void {
    if (!this.onFrame) return;
    const next = this.source.build(this.size.cols, this.size.rows, this.tick);
    const frame = buildFrame(++this.seq, next, full ? null : this.last);
    if (link) frame.link = link;
    this.last = next;
    if (full || this.title !== this.sentTitle) {
      frame.title = this.title;
      this.sentTitle = this.title;
    }
    if (this.dropNext && !full) {
      this.dropNext = false;
      return;
    }
    this.framesSent++;
    this.onFrame(frame);
  }

  /** Test hook: the program exits. */
  exit(code: number | null = 0): void {
    this.onExit?.(code);
  }

  close(): void {
    this.closed = true;
    this.dispose();
  }

  dispose(): void {
    if (this.timer) clearInterval(this.timer);
  }
}
