import { describe, expect, it } from "vitest";
import { Mods } from "../proto";
import { decideKeyDown, keyUpEvent, ModSides, type KeyLike } from "./keyboard";

function key(over: Partial<KeyLike> & { key: string; code: string }): KeyLike {
  return { repeat: false, shiftKey: false, ctrlKey: false, altKey: false, metaKey: false, isComposing: false, keyCode: 0, ...over };
}

const sides = () => new ModSides();

describe("decideKeyDown", () => {
  it("sends printable keys with their text", () => {
    expect(decideKeyDown(key({ key: "a", code: "KeyA" }), sides(), "mac")).toEqual({
      kind: "key", action: "press", code: "KeyA", key: "a", text: "a", mods: 0, composing: false,
    });
    expect(decideKeyDown(key({ key: "A", code: "KeyA", shiftKey: true }), sides(), "mac")).toMatchObject({ text: "A", mods: Mods.SHIFT });
    expect(decideKeyDown(key({ key: " ", code: "Space" }), sides(), "mac")).toMatchObject({ text: " " });
  });

  it("drops text when ctrl or super is held, and for named keys", () => {
    expect(decideKeyDown(key({ key: "c", code: "KeyC", ctrlKey: true }), sides(), "mac")).toMatchObject({ text: null, mods: Mods.CTRL });
    expect(decideKeyDown(key({ key: "k", code: "KeyK", metaKey: true }), sides(), "mac")).toMatchObject({ text: null, mods: Mods.SUPER });
    expect(decideKeyDown(key({ key: "Enter", code: "Enter" }), sides(), "mac")).toMatchObject({ text: null });
    expect(decideKeyDown(key({ key: "ArrowUp", code: "ArrowUp" }), sides(), "other")).toMatchObject({ text: null });
    expect(decideKeyDown(key({ key: "F1", code: "F1" }), sides(), "other")).toMatchObject({ text: null });
  });

  it("keeps option-produced text on macOS with the alt bit", () => {
    expect(decideKeyDown(key({ key: "å", code: "KeyA", altKey: true }), sides(), "mac")).toMatchObject({ text: "å", mods: Mods.ALT });
  });

  it("marks repeats", () => {
    expect(decideKeyDown(key({ key: "x", code: "KeyX", repeat: true }), sides(), "mac")).toMatchObject({ action: "repeat" });
  });

  it("leaves composition and dead keys to the IME path", () => {
    expect(decideKeyDown(key({ key: "a", code: "KeyA", isComposing: true }), sides(), "mac")).toBe("ime");
    expect(decideKeyDown(key({ key: "Process", code: "KeyA", keyCode: 229 }), sides(), "other")).toBe("ime");
    expect(decideKeyDown(key({ key: "Dead", code: "Quote" }), sides(), "other")).toBe("ime");
  });

  it("passes app and paste shortcuts through to the webview", () => {
    for (const code of ["KeyQ", "KeyW", "KeyH", "KeyM", "KeyV", "KeyC"]) {
      expect(decideKeyDown(key({ key: code.slice(3).toLowerCase(), code, metaKey: true }), sides(), "mac")).toBe("passthrough");
    }
    expect(decideKeyDown(key({ key: "V", code: "KeyV", ctrlKey: true, shiftKey: true }), sides(), "other")).toBe("passthrough");
    expect(decideKeyDown(key({ key: "Insert", code: "Insert", shiftKey: true }), sides(), "other")).toBe("passthrough");
    // Plain Ctrl+V is the terminal's (^V), not paste.
    expect(decideKeyDown(key({ key: "v", code: "KeyV", ctrlKey: true }), sides(), "other")).toMatchObject({ kind: "key" });
    // Cmd+V on Linux/Windows is just super+v.
    expect(decideKeyDown(key({ key: "v", code: "KeyV", metaKey: true }), sides(), "other")).toMatchObject({ kind: "key" });
  });

  it("reports which side a modifier is on", () => {
    const s = sides();
    s.update("ShiftRight", true);
    expect(decideKeyDown(key({ key: "A", code: "KeyA", shiftKey: true }), s, "mac")).toMatchObject({ mods: Mods.SHIFT | Mods.SHIFT_RIGHT });
    s.update("ShiftRight", false);
    s.update("ShiftLeft", true);
    expect(decideKeyDown(key({ key: "A", code: "KeyA", shiftKey: true }), s, "mac")).toMatchObject({ mods: Mods.SHIFT });
    s.update("ControlRight", true);
    s.update("AltRight", true);
    expect(decideKeyDown(key({ key: "x", code: "KeyX", ctrlKey: true, altKey: true }), s, "other")).toMatchObject({
      mods: Mods.CTRL | Mods.CTRL_RIGHT | Mods.ALT | Mods.ALT_RIGHT,
    });
  });

  it("reads caps lock and num lock", () => {
    const e = key({ key: "A", code: "KeyA", getModifierState: (k) => k === "CapsLock" || k === "NumLock" });
    expect(decideKeyDown(e, sides(), "mac")).toMatchObject({ mods: Mods.CAPS_LOCK | Mods.NUM_LOCK });
  });

  it("builds release events without text", () => {
    expect(keyUpEvent(key({ key: "a", code: "KeyA" }), sides())).toMatchObject({ action: "release", text: null });
  });
});

describe("scrollback shortcuts", () => {
  const d = (over: Partial<KeyLike> & { key: string; code: string }, p: "mac" | "other") => decideKeyDown(key(over), sides(), p);

  it("pages with Shift+PageUp/PageDown everywhere", () => {
    for (const p of ["mac", "other"] as const) {
      expect(d({ key: "PageUp", code: "PageUp", shiftKey: true }, p)).toEqual({ kind: "scroll", by: "pages", n: -1 });
      expect(d({ key: "PageDown", code: "PageDown", shiftKey: true }, p)).toEqual({ kind: "scroll", by: "pages", n: 1 });
    }
    expect(d({ key: "PageUp", code: "PageUp", metaKey: true }, "mac")).toEqual({ kind: "scroll", by: "pages", n: -1 });
  });

  it("jumps with Cmd+Home/End on macOS and Shift+Home/End elsewhere", () => {
    expect(d({ key: "Home", code: "Home", metaKey: true }, "mac")).toEqual({ kind: "scroll", by: "top", n: 0 });
    expect(d({ key: "End", code: "End", metaKey: true }, "mac")).toEqual({ kind: "scroll", by: "bottom", n: 0 });
    expect(d({ key: "Home", code: "Home", shiftKey: true }, "other")).toEqual({ kind: "scroll", by: "top", n: 0 });
    expect(d({ key: "End", code: "End", shiftKey: true }, "other")).toEqual({ kind: "scroll", by: "bottom", n: 0 });
  });

  it("leaves plain and other modified keys to the program", () => {
    expect(d({ key: "PageUp", code: "PageUp" }, "mac")).toMatchObject({ kind: "key" });
    expect(d({ key: "Home", code: "Home", shiftKey: true }, "mac")).toMatchObject({ kind: "key" });
    expect(d({ key: "PageUp", code: "PageUp", ctrlKey: true, shiftKey: true }, "other")).toMatchObject({ kind: "key" });
  });

  it("copies with Ctrl+Shift+C off macOS", () => {
    expect(d({ key: "C", code: "KeyC", ctrlKey: true, shiftKey: true }, "other")).toBe("copy");
    expect(d({ key: "c", code: "KeyC", metaKey: true }, "mac")).toBe("passthrough");
  });
});
