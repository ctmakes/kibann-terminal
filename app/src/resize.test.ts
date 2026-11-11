import { describe, expect, it } from "vitest";
import { computeMetrics } from "./metrics";
import { gridSize } from "./resize";

const m = computeMetrics({ advance: 8.4, ascent: 14.28, descent: 4.2 }, 14, 2);

describe("gridSize", () => {
  it("fits whole cells inside the padding", () => {
    // 800 css = 1600 dev; minus 16 padding = 1584 / 17 = 93.2
    expect(gridSize(800, 480, m)).toEqual({ cols: 93, rows: 25 });
  });

  it("never goes below 2x1", () => {
    expect(gridSize(1, 1, m)).toEqual({ cols: 2, rows: 1 });
  });
});
