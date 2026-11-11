import { describe, expect, it } from "vitest";
import { buildFrame, Screen } from "../dev/screen";
import { decodeFrame, FrameDecodeError } from "./decode";

describe("decodeFrame", () => {
  it("accepts parsed and string JSON", () => {
    const f = buildFrame(1, new Screen(3, 2), null);
    expect(decodeFrame(f)).toBe(f);
    expect(decodeFrame(JSON.stringify(f))).toEqual(f);
  });

  it("rejects malformed frames", () => {
    expect(() => decodeFrame({ seq: 1 })).toThrow(FrameDecodeError);
    expect(() => decodeFrame(null)).toThrow(FrameDecodeError);
  });
});
