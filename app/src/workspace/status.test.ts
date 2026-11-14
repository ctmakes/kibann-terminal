import { describe, expect, it } from "vitest";
import { agentName, arrivals, blockedCount, dotFor, rollUp, type StatusMap } from "./status";

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

describe("notifications", () => {
  const map = (o: Record<number, string>): StatusMap =>
    Object.fromEntries(Object.entries(o).map(([id, state]) => [id, { agent: "claude", state }])) as StatusMap;

  it("tells of an agent that starts waiting or finishes, elsewhere", () => {
    const prev = map({ 1: "working", 2: "working", 3: "blocked", 4: "working" });
    const next = map({ 1: "blocked", 2: "done", 3: "blocked", 4: "working" });
    expect(arrivals(prev, next, null).map((a) => [a.pane, a.state])).toEqual([
      [1, "blocked"],
      [2, "done"],
    ]);
  });

  it("working to idle is finishing too (the focused pane, window behind)", () => {
    const got = arrivals(map({ 1: "working", 2: "blocked" }), map({ 1: "idle", 2: "idle" }), null);
    expect(got.map((a) => [a.pane, a.state])).toEqual([[1, "done"]]);
  });

  it("not of the pane the user is looking at", () => {
    const next = map({ 1: "blocked", 2: "done" });
    expect(arrivals({}, next, 1).map((a) => a.pane)).toEqual([2]);
  });

  it("again after it went back to work", () => {
    const a = map({ 1: "blocked" });
    expect(arrivals(a, map({ 1: "working" }), null)).toEqual([]);
    expect(arrivals(map({ 1: "working" }), a, null)).toHaveLength(1);
  });

  it("names agents", () => {
    expect(agentName("claude")).toBe("Claude Code");
    expect(agentName("opencode")).toBe("OpenCode");
    expect(agentName("codex")).toBe("Codex");
    expect(agentName("")).toBe("An agent");
  });
});
