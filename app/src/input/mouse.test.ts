import { describe, expect, it } from "vitest";
import { buttonOf, linkModifier, WheelAccumulator } from "./mouse";
import { linkCovers } from "../links";

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

describe("links", () => {
  it("uses Cmd on macOS and Ctrl elsewhere", () => {
    expect(linkModifier({ metaKey: true, ctrlKey: false }, "mac")).toBe(true);
    expect(linkModifier({ metaKey: false, ctrlKey: true }, "mac")).toBe(false);
    expect(linkModifier({ metaKey: false, ctrlKey: true }, "other")).toBe(true);
    expect(linkModifier({ metaKey: true, ctrlKey: false }, "other")).toBe(false);
  });

  it("knows which cells a wrapped link covers", () => {
    const link = { uri: "u", target: "u", spans: [{ y: 2, x: 4, len: 3 }, { y: 3, x: 0, len: 2 }] };
    expect(linkCovers(link, 4, 2)).toBe(true);
    expect(linkCovers(link, 6, 2)).toBe(true);
    expect(linkCovers(link, 7, 2)).toBe(false);
    expect(linkCovers(link, 1, 3)).toBe(true);
    expect(linkCovers(link, 2, 3)).toBe(false);
    expect(linkCovers(link, 4, 3)).toBe(false);
  });
});
