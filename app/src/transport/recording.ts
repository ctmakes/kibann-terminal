import type { Frame, InputEvent, Resize } from "../proto";
import { decodeFrame } from "../proto/decode";
import type { Transport } from "./types";

/** One line of a `vt-replay` frame file: a frame and when it was sent. */
export interface TimedFrame {
  /** Milliseconds from the start of the recording. */
  t: number;
  frame: Frame;
}

export function parseFrames(jsonl: string): TimedFrame[] {
  return jsonl
    .split("\n")
    .filter((l) => l.trim())
    .map((l, i) => {
      const v = JSON.parse(l) as { t: unknown; frame: unknown };
      if (typeof v.t !== "number") throw new Error(`line ${i + 1}: no time`);
      return { t: v.t, frame: decodeFrame(v.frame) };
    });
}

export interface RecordingOptions {
  /** Send every frame up to this time at once, then stop. Omit to play in real time. */
  until?: number;
  /** Playback speed for real-time play. */
  speed?: number;
}

/**
 * Plays frames recorded from the real emulator (`vt-replay`). The recording
 * has a fixed size, so resizes are ignored and the frontend renders at the
 * recorded size. Frames are partial, so none can be skipped: playback waits
 * whenever two are unacknowledged, like the backend.
 */
export class RecordingTransport implements Transport {
  readonly inputs: InputEvent[] = [];
  readonly resizes: Resize[] = [];
  onInput?: (e: InputEvent) => void;
  fullRequests = 0;
  /** Index of the next frame to send. */
  next = 0;
  private onFrame: ((f: Frame) => void) | null = null;
  private acked = 0;
  private sent = 0;
  private start = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;

  constructor(
    readonly frames: TimedFrame[],
    private readonly opts: RecordingOptions = {},
  ) {}

  get done(): boolean {
    return this.next >= this.frames.length;
  }

  async attach(onFrame: (f: Frame) => void): Promise<void> {
    this.onFrame = onFrame;
    if (this.opts.until !== undefined) {
      const until = this.opts.until;
      while (!this.done && (this.next === 0 || this.frames[this.next].t <= until)) this.sendNext();
      return;
    }
    this.start = performance.now();
    this.pump();
  }

  private sendNext(): void {
    const f = this.frames[this.next++].frame;
    this.sent = f.seq;
    this.onFrame?.(f);
  }

  /** Send every frame that is due, then sleep until the next one. */
  private pump(): void {
    this.timer = null;
    const speed = this.opts.speed ?? 1;
    while (!this.done && this.sent - this.acked <= 1) {
      const due = this.start + this.frames[this.next].t / speed;
      const wait = due - performance.now();
      if (wait > 0) {
        this.timer = setTimeout(() => this.pump(), wait);
        return;
      }
      this.sendNext();
    }
    // Otherwise blocked on an ack, or finished; `ack` resumes.
  }

  input(e: InputEvent): void {
    this.inputs.push(e);
    this.onInput?.(e);
  }

  resize(size: Resize): void {
    this.resizes.push(size);
  }

  ack(seq: number): void {
    this.acked = Math.max(this.acked, seq);
    if (this.opts.until === undefined && !this.timer && !this.done && this.onFrame) this.pump();
  }

  /** A recording can't rebuild a full frame; frames arrive in order, so this shouldn't happen. */
  requestFull(): void {
    this.fullRequests++;
  }

  dispose(): void {
    if (this.timer) clearTimeout(this.timer);
  }
}
