import { describe, expect, it } from "vitest";
import type { Theme } from "../proto";
import { KIBANN_DARK, chromeFor, contrast, isDark, mix } from "./theme";

const num = (css: string) => Number.parseInt(css.slice(1), 16);

const LIGHT: Theme = {
  ...KIBANN_DARK,
  palette: [...KIBANN_DARK.palette],
  background: 0xffffff,
  foreground: 0x1f2328,
};
const SOLARIZED: Theme = { ...KIBANN_DARK, background: 0xfdf6e3, foreground: 0x657b83 };
const DRACULA: Theme = { ...KIBANN_DARK, background: 0x282a36, foreground: 0xf8f8f2 };

describe("chromeFor", () => {
  it("gives back the old greys on the built-in dark theme", () => {
    const v = chromeFor(KIBANN_DARK);
    expect(v["--k-sidebar"]).toBe("#101010");
    expect(v["--k-muted"]).toBe("#7d7d7d");
    expect(v["--k-bg"]).toBe("#000000");
  });

  it("knows light from dark", () => {
    expect(isDark(KIBANN_DARK)).toBe(true);
    expect(isDark(DRACULA)).toBe(true);
    expect(isDark(LIGHT)).toBe(false);
    expect(isDark(SOLARIZED)).toBe(false);
  });

  for (const [name, t] of [
    ["dark", KIBANN_DARK],
    ["light", LIGHT],
    ["Dracula", DRACULA],
  ] as const) {
    it(`keeps text readable on ${name}`, () => {
      const v = chromeFor(t);
      for (const surface of ["--k-sidebar", "--k-bar", "--k-header", "--k-panel"]) {
        for (const text of ["--k-text", "--k-strong", "--k-title", "--k-bright"]) {
          expect(contrast(num(v[text]), num(v[surface])), `${text} on ${surface}`).toBeGreaterThanOrEqual(4.5);
        }
      }
      expect(contrast(num(v["--k-current-text"]), num(v["--k-current"]))).toBeGreaterThanOrEqual(4.5);
    });
  }

  it("uses the status colors meant for the background", () => {
    expect(chromeFor(KIBANN_DARK)["--k-working"]).toBe("#3fb950");
    expect(chromeFor(LIGHT)["--k-working"]).toBe("#1a7f37");
  });

  it("mixes per channel", () => {
    expect(mix(0x000000, 0xffffff, 0.5)).toBe(0x808080);
    expect(mix(0xff0000, 0x0000ff, 1)).toBe(0x0000ff);
  });
});
