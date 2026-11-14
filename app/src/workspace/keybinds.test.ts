import { afterEach, describe, expect, it } from "vitest";
import { applyKeybinds, parseAction, parseTrigger } from "./keybinds";
import { actionFor, bindings, defaultBindings, keyFor, menuKeys, setKeybinds } from "./keymap";

const key = (code: string, mods: { meta?: boolean; shift?: boolean; ctrl?: boolean; alt?: boolean } = {}) => ({
  code,
  key: "",
  metaKey: !!mods.meta,
  shiftKey: !!mods.shift,
  ctrlKey: !!mods.ctrl,
  altKey: !!mods.alt,
  isComposing: false,
  repeat: false,
  keyCode: 0,
});

describe("keybind lines", () => {
  afterEach(() => setKeybinds("mac", []));

  it("read Ghostty's triggers", () => {
    expect(parseTrigger("super+shift+d")).toEqual({ mods: "ms", code: "KeyD" });
    expect(parseTrigger("cmd+bracket_left")).toEqual({ mods: "m", code: "BracketLeft" });
    expect(parseTrigger("ctrl+alt+arrow_up")).toEqual({ mods: "ac", code: "ArrowUp" });
    expect(parseTrigger("super+=")).toEqual({ mods: "m", code: "Equal" });
    expect(parseTrigger("super++")).toEqual({ mods: "ms", code: "Equal" });
    expect(parseTrigger("super+digit_1")).toEqual({ mods: "m", code: "Digit1" });
    expect(parseTrigger("f3")).toEqual({ mods: "", code: "F3" });
    expect(parseTrigger("hyper+a")).toBe('unknown modifier "hyper"');
    expect(parseTrigger("ctrl+a>n")).toBe("key sequences aren't supported");
    expect(parseTrigger("global:super+a")).toBe('"global:" isn\'t supported');
  });

  it("read Ghostty's actions", () => {
    expect(parseAction("new_split:down")).toEqual({ kind: "split", dir: "column" });
    expect(parseAction("goto_split:next")).toEqual({ kind: "focusCycle", step: 1 });
    expect(parseAction("goto_split:top")).toEqual({ kind: "focus", toward: "up" });
    expect(parseAction("resize_split:left,20")).toEqual({ kind: "resize", toward: "left" });
    expect(parseAction("goto_tab:3")).toEqual({ kind: "goToTab", index: 2 });
    expect(parseAction("navigate_search:previous")).toEqual({ kind: "findStep", older: false });
    expect(parseAction("toggle_sidebar")).toEqual({ kind: "sidebar" });
    expect(parseAction("open_settings")).toEqual({ kind: "settings" });
    expect(parseAction("unbind")).toBe("unbind");
    expect(parseAction("toggle_quick_terminal")).toBe('unknown action "toggle_quick_terminal"');
  });

  it("rebind, unbind and clear over the defaults, in order", () => {
    const d = defaultBindings("mac");
    const { table, errors } = applyKeybinds(d, [
      "super+d=new_split:down",
      "super+t=unbind",
      "super+k=nonsense",
    ]);
    expect(errors).toEqual(['keybind = super+k=nonsense: unknown action "nonsense"']);
    expect(table.find((b) => b.mods === "m" && b.code === "KeyD")?.action).toEqual({ kind: "split", dir: "column" });
    expect(table.some((b) => b.code === "KeyT" && b.mods === "m")).toBe(false);
    expect(table).toHaveLength(d.length - 1);
    const cleared = applyKeybinds(d, ["clear", "super+n=new_tab"]).table;
    expect(cleared).toEqual([{ mods: "m", code: "KeyN", action: { kind: "newTab" } }]);
  });

  it("change what keys do, what's shown and the menu", () => {
    expect(setKeybinds("mac", ["super+shift+s=toggle_sidebar", "super+shift+b=unbind"])).toEqual([]);
    expect(actionFor(key("KeyS", { meta: true, shift: true }), "mac")).toEqual({ kind: "sidebar" });
    expect(actionFor(key("KeyB", { meta: true, shift: true }), "mac")).toBeNull();
    expect(keyFor({ kind: "sidebar" }, "mac")).toBe("⇧⌘S");
    const menu = menuKeys("mac");
    expect(menu['{"kind":"sidebar"}']).toBe("Cmd+Shift+KeyS");
    expect(menu['{"kind":"newTab"}']).toBe("Cmd+KeyT");
    setKeybinds("mac", ["super+t=unbind"]);
    expect(menuKeys("mac")['{"kind":"newTab"}']).toBeNull();
    setKeybinds("mac", []);
    expect(bindings("mac")).toEqual(defaultBindings("mac"));
    expect(actionFor(key("KeyB", { meta: true, shift: true }), "mac")).toEqual({ kind: "sidebar" });
  });
});
