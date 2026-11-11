import { describe, expect, it } from "vitest";
import { buttonOf, WheelAccumulator } from "./mouse";

describe("buttonOf", () => {
  it("maps DOM buttons to libghostty buttons", () => {
    expect([0, 1, 2, 3, 4, 5].map(buttonOf)).toEqual([1, 3, 2, 8, 9, null]);
  });
});

describe("WheelAccumulator", () => {
  it("accumulates pixel deltas into whole rows", () => {
    const w = new WheelAccumulator();
    expect(w.push(0, 10, 0, 8, 18, 24)).toEqual({ dx: 0, dy: 0 });
    expect(w.push(0, 10, 0, 8, 18, 24)).toEqual({ dx: 0, dy: 1 });
    expect(w.push(0, 34, 0, 8, 18, 24)).toEqual({ dx: 0, dy: 2 });
  });

  it("handles negative and horizontal deltas", () => {
    const w = new WheelAccumulator();
    expect(w.push(-17, -40, 0, 8, 18, 24)).toEqual({ dx: -2, dy: -2 });
  });

  it("treats line and page modes as rows", () => {
    const w = new WheelAccumulator();
    expect(w.push(0, 3, 1, 8, 18, 24)).toEqual({ dx: 0, dy: 3 });
    expect(w.push(0, 1, 2, 8, 18, 24)).toEqual({ dx: 0, dy: 24 });
  });
});
