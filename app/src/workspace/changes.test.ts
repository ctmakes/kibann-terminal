import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { parseChangelog } from "./changes";

describe("parseChangelog", () => {
  it("reads sections, paragraphs, wrapped list items and subheadings", () => {
    const r = parseChangelog(
      [
        "# Changelog",
        "",
        "About this file, left out.",
        "",
        "## [0.2.0] - Unreleased",
        "",
        "One paragraph,",
        "wrapped.",
        "",
        "### Fixed",
        "- First item",
        "  continues here.",
        "- Second with `code`",
        "",
        "## [0.1.1] - Included in 0.2.0",
        "- Small fix",
        "## [0.1.0] - 2026-11-14",
        "Plain.",
      ].join("\n"),
    );
    expect(r.map((x) => [x.version, x.when])).toEqual([
      ["0.2.0", "Unreleased"],
      ["0.1.1", "Included in 0.2.0"],
      ["0.1.0", "2026-11-14"],
    ]);
    expect(r[0]!.blocks).toEqual([
      { t: "p", text: "One paragraph, wrapped." },
      { t: "h", text: "Fixed" },
      { t: "list", items: ["First item continues here.", "Second with `code`"] },
    ]);
    expect(r[1]!.blocks).toEqual([{ t: "list", items: ["Small fix"] }]);
    expect(r[2]!.blocks).toEqual([{ t: "p", text: "Plain." }]);
  });

  it("reads the real CHANGELOG.md", () => {
    const r = parseChangelog(readFileSync(new URL("../../../CHANGELOG.md", import.meta.url), "utf8"));
    expect(r.length).toBeGreaterThan(0);
    expect(r[0]!.version).toMatch(/^\d+\.\d+\.\d+$/);
    expect(r[0]!.blocks.length).toBeGreaterThan(0);
  });
});
