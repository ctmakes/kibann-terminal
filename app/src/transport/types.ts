import type { Frame, InputEvent, Resize } from "../proto";

/** How the renderer talks to whatever owns the terminal. */
export interface Transport {
  /** Start receiving frames. The first one must be full. */
  attach(onFrame: (frame: Frame) => void): Promise<void>;
  input(event: InputEvent): void;
  resize(size: Resize): void;
  /** The frame with this seq has been painted. */
  ack(seq: number): void;
  requestFull(): void;
  /** The program ended on its own (test transports; the daemon closes the
   * pane itself). Set by the owner before `attach`. */
  onExit?: (code: number | null) => void;
  /** The pane is gone from the layout: stop showing the session. */
  close?(): void;
}
