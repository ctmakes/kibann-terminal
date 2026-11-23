// The daemon's layout model writes these cases (crates/daemon/src/layout.rs,
// `the_shared_cases_match`): every call's result and the layout after it.
// This model must give the same, step for step.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import type { Layout, Tree } from "../proto";
import type { Dir, Path, PaneId } from "./layout";
import { Model, type TabId } from "./model";

type Op =
  | { op: "new_tab"; pane: PaneId; focus: boolean }
  | { op: "add_tab"; tree: Tree; focused: PaneId; title: string | null; show: boolean }
  | { op: "split"; target: PaneId; dir: Dir; pane: PaneId; focus: boolean }
  | { op: "remove"; pane: PaneId }
  | { op: "focus"; pane: PaneId }
  | { op: "activate"; tab: TabId }
  | { op: "set_ratio"; tab: TabId; path: Path; ratio: number }
  | { op: "equalize"; tab: TabId }
  | { op: "zoom"; tab: TabId; pane: PaneId | null };

type Step = Op & { result: unknown; layout: Layout };

function run(m: Model, s: Op): unknown {
  switch (s.op) {
    case "new_tab":
      return m.newTab(s.pane, s.focus);
    case "add_tab":
      return m.addTab(s.tree, s.focused, s.title, s.show);
    case "split":
      return m.split(s.target, s.dir, s.pane, s.focus);
    case "remove":
      return m.remove(s.pane);
    case "focus":
      return m.focus(s.pane);
    case "activate":
      return m.activate(s.tab);
    case "set_ratio":
      return m.setRatio(s.tab, s.path, s.ratio);
    case "equalize":
      return m.equalize(s.tab);
    case "zoom":
      return m.zoom(s.tab, s.pane);
  }
}

const FILE = fileURLToPath(new URL("../../../fixtures/layout/cases.jsonl", import.meta.url));
const CASES = readFileSync(FILE, "utf8")
  .split("\n")
  .filter((l) => l.trim())
  .map((l) => JSON.parse(l) as { name: string; steps: Step[] });

describe("layout model (fixtures/layout, from the daemon's)", () => {
  for (const c of CASES) {
    it(c.name, () => {
      const m = new Model();
      c.steps.forEach(({ result, layout, ...op }, i) => {
        const what = `step ${i + 1}: ${JSON.stringify(op)}`;
        expect(run(m, op), what).toEqual(result);
        expect(m.layout, what).toEqual(layout);
      });
    });
  }
});
