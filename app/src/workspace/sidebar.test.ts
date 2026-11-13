import { describe, expect, it } from "vitest";
import type { PaneDir } from "../proto";
import { groupPanes, type CardPane } from "./sidebar";

const pane = (id: number, tab: number): CardPane => ({
  id,
  title: `p${id}`,
  tab,
  shown: false,
  focused: false,
  canvas: null,
  bg: "#000",
  cursor: () => 1,
});
const inRepo = (repo: string): PaneDir => ({
  dir: `/r/${repo}`,
  shown: `/r/${repo}`,
  git: {
    root: `/r/${repo}`,
    repo,
    worktree: null,
    branch: "main",
    detached: null,
    upstream: null,
    ahead: 0,
    behind: 0,
    changed: 0,
    conflicts: 0,
    operation: null,
  },
});

describe("sidebar groups", () => {
  const panes = [pane(1, 0), pane(2, 0), pane(3, 1), pane(4, 1), pane(5, 2)];
  const git = {
    "1": inRepo("alpha"),
    "2": { dir: "/Users/x", shown: "~", git: null },
    "3": inRepo("beta"),
    "4": inRepo("alpha"),
  };

  it("groups by repository in order of first appearance", () => {
    const g = groupPanes(panes, git, {});
    expect(g.map((x) => [x.label, x.panes.map((p) => p.id)])).toEqual([
      ["alpha", [1, 4]],
      ["~", [2]],
      ["beta", [3]],
      ["…", [5]],
    ]);
  });

  it("puts groups needing attention first, keeping pane order", () => {
    const g = groupPanes(panes, git, {
      "3": { agent: "claude", state: "working" },
      "2": { agent: "claude", state: "done" },
      "4": { agent: "claude", state: "blocked" },
    });
    expect(g.map((x) => x.label)).toEqual(["alpha", "~", "beta", "…"]);
    const g2 = groupPanes(panes, git, { "3": { agent: "claude", state: "blocked" } });
    expect(g2.map((x) => x.label)).toEqual(["beta", "alpha", "~", "…"]);
  });
});
