import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { DEFAULT_SCREEN } from "./default";

/** A node as serde reads it: defaults filled in. */
function normalize(n: any): any {
  return {
    ...n,
    resizable: n.resizable ?? false,
    hidden: n.hidden ?? false,
    children: (n.children ?? []).map(normalize),
    style: n.style ?? {},
  };
}

describe("the default screen", () => {
  it("is screens/default/screen.json", () => {
    const file = JSON.parse(readFileSync(new URL("../../../screens/default/screen.json", import.meta.url), "utf8"));
    expect(DEFAULT_SCREEN.screen.about).toBe(file.about);
    expect(normalize(DEFAULT_SCREEN.screen.root)).toEqual(normalize(file.root));
    expect(file.floating ?? []).toEqual([]);
    expect(file.chrome ?? { colors: {} }).toEqual({ colors: {}, ...(file.chrome ?? {}) });
  });
});
