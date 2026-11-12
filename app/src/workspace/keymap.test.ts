import { describe, expect, it } from "vitest";
import type { KeyLike } from "../input/keyboard";
import { actionFor } from "./keymap";

function key(code: string, mods: string): KeyLike {
  return {
    key: "", code, repeat: false, isComposing: false, keyCode: 0,
    ctrlKey: mods.includes("c"), altKey: mods.includes("a"), metaKey: mods.includes("m"), shiftKey: mods.includes("s"),
  };
}

describe("macOS", () => {
  const a = (code: string, mods: string) => actionFor(key(code, mods), "mac");
  it("maps Ghostty's shortcuts", () => {
    expect(a("KeyT", "m")).toEqual({ kind: "newTab" });
    expect(a("KeyW", "m")).toEqual({ kind: "close" });
    expect(a("KeyD", "m")).toEqual({ kind: "split", dir: "row" });
    expect(a("KeyD", "ms")).toEqual({ kind: "split", dir: "column" });
    expect(a("ArrowLeft", "am")).toEqual({ kind: "focus", toward: "left" });
    expect(a("ArrowUp", "cm")).toEqual({ kind: "resize", toward: "up" });
    expect(a("Enter", "ms")).toEqual({ kind: "zoom" });
    expect(a("BracketRight", "m")).toEqual({ kind: "focusCycle", step: 1 });
    expect(a("BracketRight", "ms")).toEqual({ kind: "tabCycle", step: 1 });
    expect(a("Digit3", "m")).toEqual({ kind: "goToTab", index: 2 });
    expect(a("Digit9", "m")).toEqual({ kind: "goToTab", index: -1 });
    expect(a("Tab", "c")).toEqual({ kind: "tabCycle", step: 1 });
  });
  it("needs exactly those modifiers", () => {
    expect(a("KeyD", "cm")).toBeNull();
    expect(a("KeyT", "")).toBeNull();
    expect(a("KeyC", "m")).toBeNull(); // copy stays with the webview
  });
});

describe("Linux and Windows", () => {
  const a = (code: string, mods: string) => actionFor(key(code, mods), "other");
  it("maps Ghostty's shortcuts", () => {
    expect(a("KeyT", "cs")).toEqual({ kind: "newTab" });
    expect(a("KeyO", "cs")).toEqual({ kind: "split", dir: "row" });
    expect(a("KeyE", "cs")).toEqual({ kind: "split", dir: "column" });
    expect(a("ArrowRight", "ca")).toEqual({ kind: "focus", toward: "right" });
    expect(a("PageDown", "c")).toEqual({ kind: "tabCycle", step: 1 });
    expect(a("Digit1", "a")).toEqual({ kind: "goToTab", index: 0 });
  });
  it("leaves terminal keys alone", () => {
    expect(a("KeyT", "c")).toBeNull(); // ^T
    expect(a("KeyD", "m")).toBeNull();
  });
});
