import { describe, expect, it } from "vitest";
import { blockedCount, dotFor, rollUp, type StatusMap } from "./status";

const map: StatusMap = {
  "1": { agent: "claude", state: "working" },
  "2": { agent: "claude", state: "done" },
  "3": { agent: "claude", state: "blocked" },
  "4": { agent: "claude", state: "idle" },
};

describe("agent status", () => {
  it("shows no dot for idle panes and plain shells", () => {
    expect(dotFor(map, 4)).toBe(null);
    expect(dotFor(map, 9)).toBe(null);
    expect(dotFor(map, 1)).toBe("working");
  });

  it("rolls a tab up to its most urgent pane", () => {
    expect(rollUp(map, [1, 2, 3])).toBe("blocked");
    expect(rollUp(map, [1, 2])).toBe("done");
    expect(rollUp(map, [4, 1])).toBe("working");
    expect(rollUp(map, [4, 9])).toBe(null);
  });

  it("counts blocked panes", () => {
    expect(blockedCount(map)).toBe(1);
    expect(blockedCount({})).toBe(0);
  });
});
