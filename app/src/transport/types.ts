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
}
