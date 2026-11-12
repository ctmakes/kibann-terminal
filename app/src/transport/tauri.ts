import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Frame, InputEvent, Resize } from "../proto";
import { decodeFrame } from "../proto/decode";
import type { Transport } from "./types";

export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export type SessionId = number;

/** Sessions still running in the backend (after a page reload). */
export function liveSessions(): Promise<SessionId[]> {
  return invoke<SessionId[]>("term_list");
}

const exitHandlers = new Map<SessionId, (code: number | null) => void>();
let listening: Promise<unknown> | null = null;

function listenForExits(): Promise<unknown> {
  listening ??= listen<{ id: SessionId; code: number | null }>("term-exited", (e) => {
    exitHandlers.get(e.payload.id)?.(e.payload.code);
  });
  return listening;
}

export interface TauriTransportOptions {
  /** Reattach to this running session instead of starting one. */
  id?: SessionId;
  /** Start the new shell in this session's working directory. */
  cwdFrom?: SessionId;
}

/**
 * One session over Tauri: frames over a Channel, input as commands. A new
 * session starts on `attach`, at the size the view reported by then.
 */
export class TauriTransport implements Transport {
  id: SessionId | null;
  onExit?: (code: number | null) => void;
  private size: Resize | null = null;
  private closed = false;

  constructor(private readonly opts: TauriTransportOptions = {}) {
    this.id = opts.id ?? null;
  }

  async attach(onFrame: (frame: Frame) => void): Promise<void> {
    await listenForExits();
    if (this.id === null) {
      const size = this.size ?? { cols: 80, rows: 24, cell_w: 8, cell_h: 16 };
      this.id = await invoke<SessionId>("term_spawn", { size, cwdFrom: this.opts.cwdFrom ?? null });
    } else if (this.size) {
      this.resize(this.size);
    }
    const id = this.id;
    exitHandlers.set(id, (code) => this.onExit?.(code));
    const channel = new Channel<unknown>();
    channel.onmessage = (raw) => onFrame(decodeFrame(raw));
    await invoke("term_attach", { id, channel });
    if (this.closed) this.close();
  }

  input(event: InputEvent): void {
    if (this.id !== null) void invoke("term_input", { id: this.id, event });
  }

  resize(size: Resize): void {
    this.size = size;
    if (this.id !== null) void invoke("term_resize", { id: this.id, size });
  }

  ack(seq: number): void {
    if (this.id !== null) void invoke("term_ack", { id: this.id, seq });
  }

  requestFull(): void {
    if (this.id !== null) void invoke("term_request_full", { id: this.id });
  }

  close(): void {
    this.closed = true;
    if (this.id === null) return; // closed while starting: attach finishes the job
    exitHandlers.delete(this.id);
    void invoke("term_close", { id: this.id });
  }
}
