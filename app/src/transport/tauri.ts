import { Channel, invoke } from "@tauri-apps/api/core";
import type { Frame, InputEvent, Resize } from "../proto";
import { decodeFrame } from "../proto/decode";
import type { Transport } from "./types";

export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Frames over a Tauri Channel; input as commands. */
export class TauriTransport implements Transport {
  async attach(onFrame: (frame: Frame) => void): Promise<void> {
    const channel = new Channel<unknown>();
    channel.onmessage = (raw) => onFrame(decodeFrame(raw));
    await invoke("term_attach", { channel });
  }

  input(event: InputEvent): void {
    void invoke("term_input", { event });
  }

  resize(size: Resize): void {
    void invoke("term_resize", { size });
  }

  ack(seq: number): void {
    void invoke("term_ack", { seq });
  }

  requestFull(): void {
    void invoke("term_request_full");
  }
}
