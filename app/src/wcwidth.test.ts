import { describe, expect, it } from "vitest";
import { graphemes, graphemeWidth, stringWidth } from "./wcwidth";

describe("wcwidth", () => {
  it("measures common cases", () => {
    expect(stringWidth("abc")).toBe(3);
    expect(stringWidth("日本")).toBe(4);
    expect(stringWidth("ｱ")).toBe(1); // halfwidth katakana
    expect(stringWidth("Ａ")).toBe(2);
    expect(graphemeWidth("é")).toBe(1);
    expect(graphemeWidth("😀")).toBe(2);
    expect(graphemeWidth("❤️")).toBe(2);
    expect(graphemeWidth("👨‍👩‍👧")).toBe(2);
    expect(graphemeWidth("🇯🇵")).toBe(2);
  });

  it("splits grapheme clusters", () => {
    expect(graphemes("éx👍🏽")).toEqual(["é", "x", "👍🏽"]);
  });
});
