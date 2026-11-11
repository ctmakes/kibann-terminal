import { describe, expect, it } from "vitest";
import { computeMetrics, fontString } from "./metrics";

// JetBrains Mono at 14px: advance 0.6em, ascent 1.02em, descent 0.3em.
const raw = { advance: 8.4, ascent: 14.28, descent: 4.2 };

describe("computeMetrics", () => {
  it("rounds cell sizes to whole device pixels", () => {
    const m1 = computeMetrics(raw, 14, 1);
    expect(m1).toMatchObject({ cellW: 8, cellH: 18, lineW: 1, pad: 4 });
    const m2 = computeMetrics(raw, 14, 2);
    expect(m2).toMatchObject({ cellW: 17, cellH: 37, lineW: 2, pad: 8 });
  });

  it("keeps the baseline and decorations inside the cell", () => {
    for (const dpr of [1, 1.25, 1.5, 2, 3]) {
      const m = computeMetrics(raw, 14, dpr);
      expect(m.baseline).toBeGreaterThan(0);
      expect(m.baseline).toBeLessThan(m.cellH);
      expect(m.underlineY).toBeGreaterThanOrEqual(m.baseline);
      expect(m.underlineY + 3 * m.lineW).toBeLessThanOrEqual(m.cellH);
      expect(m.strikeY).toBeGreaterThan(0);
      expect(m.strikeY).toBeLessThan(m.baseline);
      for (const v of [m.cellW, m.cellH, m.baseline, m.lineW, m.underlineY, m.strikeY, m.pad]) {
        expect(Number.isInteger(v)).toBe(true);
      }
    }
  });

  it("scales the canvas font by dpr", () => {
    expect(fontString({ dpr: 2, fontSize: 14 }, true, true)).toMatch(/^italic 700 28px "JetBrains Mono"/);
    expect(fontString({ dpr: 1, fontSize: 14 }, false, false)).toMatch(/^400 14px /);
  });
});
