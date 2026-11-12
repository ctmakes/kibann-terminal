import { describe, expect, it } from "vitest";
import type { GitInfo } from "../proto";
import { branchOf, headerParts } from "./pane";

const git = (g: Partial<GitInfo>): GitInfo => ({
  root: "/r/proj",
  repo: "proj",
  worktree: null,
  branch: "main",
  detached: null,
  upstream: null,
  ahead: 0,
  behind: 0,
  changed: 0,
  conflicts: 0,
  operation: null,
  ...g,
});
const text = (p: ReturnType<typeof headerParts>) => p.map((x) => x.text).join(" ");

describe("pane header", () => {
  it("shows the directory outside a repo", () => {
    expect(text(headerParts({ dir: "/Users/x/dl", shown: "~/dl", git: null }))).toBe("~/dl");
    expect(headerParts(undefined)).toEqual([]);
  });

  it("shows repo, branch and counts, hiding zeros", () => {
    const place = (g: Partial<GitInfo>) => ({ dir: "/r/proj", shown: "/r/proj", git: git(g) });
    expect(text(headerParts(place({})))).toBe("proj ⎇ main");
    expect(text(headerParts(place({ changed: 5, ahead: 2, behind: 1 })))).toBe("proj ⎇ main ●5 ↑2 ↓1");
    expect(text(headerParts(place({ behind: 3 })))).toBe("proj ⎇ main ↓3");
    expect(text(headerParts(place({ worktree: "fix-links", branch: "fix/links", changed: 1 })))).toBe(
      "proj › fix-links ⎇ fix/links ●1",
    );
    expect(text(headerParts(place({ branch: null, detached: "9f12549" })))).toBe("proj ⎇ 9f12549");
    expect(text(headerParts(place({ operation: "rebase", conflicts: 2, changed: 2 })))).toBe(
      "proj ⎇ main REBASING !2 ●2",
    );
  });

  it("gives tabs the branch", () => {
    expect(branchOf({ dir: "/", shown: "/", git: null })).toBe(null);
    expect(branchOf({ dir: "/r", shown: "/r", git: git({ branch: "dev" }) })).toBe("dev");
  });
});
