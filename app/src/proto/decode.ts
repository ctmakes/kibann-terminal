import type { Frame } from "./index";

export class FrameDecodeError extends Error {}

/**
 * The only place that knows the wire format. Today frames arrive as JSON that
 * Tauri has already parsed, so this checks the shape cheaply and returns it.
 * A binary encoding would replace the body of this function and nothing else.
 */
export function decodeFrame(raw: unknown): Frame {
  if (typeof raw === "string") raw = JSON.parse(raw);
  const f = raw as Frame;
  if (
    !f ||
    typeof f.seq !== "number" ||
    typeof f.full !== "boolean" ||
    typeof f.cols !== "number" ||
    typeof f.rows_total !== "number" ||
    !Array.isArray(f.styles) ||
    !Array.isArray(f.rows) ||
    !f.cursor
  ) {
    throw new FrameDecodeError("malformed frame");
  }
  return f;
}
