import { describe, expect, it } from "vitest";
import { crop, cursorAt } from "./thumbnail";

describe("thumbnails", () => {
  it("show all of a pane that fits", () => {
    expect(crop({ width: 400, height: 200 }, { width: 200, height: 100 }, 0.5, 1)).toEqual({ sy: 0, sw: 400, sh: 200 });
  });

  it("show the left part and the rows around the cursor", () => {
    // 1000 source rows, 200 shown: the cursor at 600 sits 70% down.
    expect(crop({ width: 800, height: 1000 }, { width: 200, height: 100 }, 0.5, 0.6)).toEqual({ sy: 460, sw: 400, sh: 200 });
    // Clamped to the top and the bottom.
    expect(crop({ width: 800, height: 1000 }, { width: 200, height: 100 }, 0.5, 0.05).sy).toBe(0);
    expect(crop({ width: 800, height: 1000 }, { width: 200, height: 100 }, 0.5, 1).sy).toBe(800);
  });

  it("put the cursor at the bottom of its row", () => {
    expect(cursorAt({ rows: 10, cursor: { y: 4 } })).toBe(0.5);
    expect(cursorAt({ rows: 0, cursor: { y: 0 } })).toBe(1);
  });
});
