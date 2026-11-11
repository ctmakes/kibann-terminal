import { describe, expect, it } from "vitest";
import { Flag } from "../proto";
import { buildFrame, Screen, style } from "./screen";

describe("buildFrame", () => {
  it("run-length encodes by style and omits blank text", () => {
    const s = new Screen(8, 1);
    s.write(0, 0, "ab", style(0x111111));
    s.write(2, 0, "cd", style(0x222222, null, Flag.BOLD));
    const f = buildFrame(1, s, null);
    expect(f.full).toBe(true);
    expect(f.rows[0].runs).toEqual([
      { x: 0, len: 2, s: 0, text: ["a", "b"] },
      { x: 2, len: 2, s: 1, text: ["c", "d"] },
      { x: 4, len: 4, s: 2 },
    ]);
    expect(f.styles[1]).toEqual({ fg: 0x222222, bg: null, ul: null, flags: Flag.BOLD });
  });

  it("marks wide cells and emits their tails as empty", () => {
    const s = new Screen(4, 1);
    s.write(0, 0, "日a");
    const run = buildFrame(1, s, null).rows[0].runs[0];
    expect(run.text).toEqual(["日", "", "a", ""]);
    expect(run.wide).toEqual([0]);
  });

  it("writes a blank spacer when a wide char doesn't fit at the end", () => {
    const s = new Screen(3, 1);
    s.write(2, 0, "日");
    expect(s.cells[0][2]).toMatchObject({ text: "", width: 1 });
  });

  it("sends only changed rows in a partial frame", () => {
    const a = new Screen(4, 3);
    const b = a.clone();
    b.write(0, 1, "x");
    const f = buildFrame(2, b, a);
    expect(f.full).toBe(false);
    expect(f.rows.map((r) => r.y)).toEqual([1]);
  });
});
