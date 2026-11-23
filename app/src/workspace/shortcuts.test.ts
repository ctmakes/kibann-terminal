import { describe, expect, it } from "vitest";
import { sheetFor } from "./shortcuts";

const rows = (p: "mac" | "other") =>
  Object.fromEntries(sheetFor(p).flatMap((g) => g.rows.map((r) => [r.label, r.keys])));

describe("shortcut sheet", () => {
  it("shows macOS keys from the keymap, collapsing arrows and digits", () => {
    const r = rows("mac");
    expect(r["New tab"]).toEqual(["⌘T"]);
    expect(r["Sidebar"]).toEqual(["⇧⌘B"]);
    expect(r["Keyboard shortcuts"]).toEqual(["⌘/"]);
    expect(r["Split right / down"]).toEqual(["⌘D / ⇧⌘D"]);
    expect(r["Pane in a direction"]).toEqual(["⌥⌘ Arrows"]);
    expect(r["Resize pane"]).toEqual(["⌃⌘ Arrows"]);
    expect(r["Tab 1–8"]).toEqual(["⌘1–8"]);
    expect(r["Last tab"]).toEqual(["⌘9"]);
    // Every binding of an action is listed: Ctrl+Tab too.
    expect(r["Previous / next tab"]).toEqual(["⇧⌘[ / ⇧⌘]", "⌃⇧Tab / ⌃Tab"]);
    expect(r["Delete the line"]).toEqual(["⌘⌫"]);
  });

  it("spells out modifiers elsewhere", () => {
    const r = rows("other");
    expect(r["New tab"]).toEqual(["Ctrl+Shift+T"]);
    expect(r["Keyboard shortcuts"]).toEqual(["Ctrl+Shift+/"]);
    expect(r["Pane in a direction"]).toEqual(["Ctrl+Alt+Arrows"]);
    expect(r["Tab 1–8"]).toEqual(["Alt+1–8"]);
    expect(r["Previous / next tab"]).toEqual(["Ctrl+PgUp / Ctrl+PgDn", "Ctrl+Shift+Tab / Ctrl+Tab"]);
    expect(r["Delete the line"]).toBeUndefined();
  });

  it("lists every workspace shortcut", () => {
    for (const p of ["mac", "other"] as const) {
      for (const g of sheetFor(p)) for (const r of g.rows) expect(r.keys.length, r.label).toBeGreaterThan(0);
    }
  });
});
