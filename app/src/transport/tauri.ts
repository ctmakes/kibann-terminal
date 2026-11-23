import { Channel } from "@tauri-apps/api/core";
import { invoke } from "../ipc";
import type { Frame, InputEvent, Resize } from "../proto";
import { decodeFrame } from "../proto/decode";
import type { Transport } from "./types";

export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export type SessionId = number;

/**
 * One session (pane) over Tauri: frames over a Channel, input as commands.
 * The daemon starts sessions; this only shows one.
 */
export class TauriTransport implements Transport {
  private released = false;
  private onFrame: ((frame: Frame) => void) | null = null;
  private channel: Channel<unknown> | null = null;

  constructor(readonly id: SessionId) {}

  async attach(onFrame: (frame: Frame) => void): Promise<void> {
    this.onFrame = onFrame;
    await this.open();
  }

  /**
   * Frames over a fresh channel, starting with a full one. A channel can
   * wedge for good: a large message goes by a fetch, and when that fails
   * (as around sleep) the channel holds every later message, waiting in
   * order for the lost one.
   */
  async reattach(): Promise<void> {
    if (this.onFrame && !this.released) await this.open();
  }

  private async open(): Promise<void> {
    const onFrame = this.onFrame!;
    const channel = new Channel<unknown>();
    if (this.channel) this.channel.onmessage = () => {};
    this.channel = channel;
    channel.onmessage = (raw) => {
      if (this.channel === channel) onFrame(decodeFrame(raw));
    };
    await invoke("term_attach", { id: this.id, channel });
    if (this.released) this.close();
  }

  input(event: InputEvent): void {
    void invoke("term_input", { id: this.id, event });
  }

  resize(size: Resize): void {
    void invoke("term_resize", { id: this.id, size });
  }

  ack(seq: number): void {
    void invoke("term_ack", { id: this.id, seq });
  }

  requestFull(): void {
    void invoke("term_request_full", { id: this.id });
  }

  /** The pane left the layout: stop routing its frames here. */
  close(): void {
    this.released = true;
    void invoke("term_release", { id: this.id });
  }
}
