// `keybind` lines from the config file, in Ghostty's syntax and with
// Ghostty's action names, applied over the default shortcuts:
//
//   keybind = super+shift+d=new_split:down
//   keybind = ctrl+a=unbind
//   keybind = clear                 (drop every default first)

import type { Direction } from "./layout";
import type { Action, Binding } from "./keymap";

const MODS: Record<string, string> = {
  super: "m",
  cmd: "m",
  command: "m",
  ctrl: "c",
  control: "c",
  alt: "a",
  opt: "a",
  option: "a",
  shift: "s",
};

const NAMED: Record<string, string> = {
  equal: "Equal",
  minus: "Minus",
  bracket_left: "BracketLeft",
  bracket_right: "BracketRight",
  slash: "Slash",
  backslash: "Backslash",
  comma: "Comma",
  period: "Period",
  semicolon: "Semicolon",
  quote: "Quote",
  backquote: "Backquote",
  grave_accent: "Backquote",
  arrow_left: "ArrowLeft",
  arrow_right: "ArrowRight",
  arrow_up: "ArrowUp",
  arrow_down: "ArrowDown",
  left: "ArrowLeft",
  right: "ArrowRight",
  up: "ArrowUp",
  down: "ArrowDown",
  enter: "Enter",
  return: "Enter",
  tab: "Tab",
  escape: "Escape",
  space: "Space",
  backspace: "Backspace",
  delete: "Delete",
  home: "Home",
  end: "End",
  page_up: "PageUp",
  page_down: "PageDown",
  "=": "Equal",
  "-": "Minus",
  "[": "BracketLeft",
  "]": "BracketRight",
  "/": "Slash",
  "\\": "Backslash",
  ",": "Comma",
  ".": "Period",
  ";": "Semicolon",
  "'": "Quote",
  "`": "Backquote",
};

/** A key name as `KeyboardEvent.code`, or null. */
function codeOf(name: string): string | null {
  const n = name.toLowerCase();
  if (/^[a-z]$/.test(n)) return `Key${n.toUpperCase()}`;
  const digit = /^(?:digit_)?([0-9])$/.exec(n);
  if (digit) return `Digit${digit[1]}`;
  const f = /^f([1-9]|1[0-2])$/.exec(n);
  if (f) return `F${f[1]}`;
  return NAMED[n] ?? null;
}

/** A trigger (`super+shift+d`) as modifiers and code, or an error. */
export function parseTrigger(t: string): { mods: string; code: string } | string {
  if (t.includes(">")) return "key sequences aren't supported";
  const prefix = /^(global|all|unconsumed|performable):/.exec(t);
  if (prefix) return `"${prefix[1]}:" isn't supported`;
  // "+" may itself be the key: ctrl++.
  const parts = t.endsWith("++") ? [...t.slice(0, -2).split("+"), "+"] : t.split("+");
  const key = parts.pop() ?? "";
  let mods = "";
  for (const p of parts) {
    const m = MODS[p.trim().toLowerCase()];
    if (!m) return `unknown modifier "${p}"`;
    if (!mods.includes(m)) mods += m;
  }
  const code = key === "+" ? "Equal" : codeOf(key.trim());
  if (!code) return `unknown key "${key}"`;
  // "+" is Shift+= on the keyboards these names assume.
  if (key === "+" && !mods.includes("s")) mods += "s";
  return { mods: [...mods].sort().join(""), code };
}

const DIRECTIONS: Record<string, Direction> = { left: "left", right: "right", up: "up", down: "down", top: "up", bottom: "down" };

/** A Ghostty action (`new_split:right`) as ours, `"unbind"`, or an error. */
export function parseAction(a: string): Action | "unbind" | string {
  const [name, arg = ""] = a.split(/:(.*)/s, 2).map((x) => x.trim());
  switch (name) {
    case "unbind":
      return "unbind";
    case "new_tab":
      return { kind: "newTab" };
    case "close_surface":
      return { kind: "close" };
    case "new_split":
      if (arg === "right") return { kind: "split", dir: "row" };
      if (arg === "down") return { kind: "split", dir: "column" };
      return `new_split:${arg}: only right and down`;
    case "goto_split":
      if (arg === "previous" || arg === "next") return { kind: "focusCycle", step: arg === "next" ? 1 : -1 };
      if (DIRECTIONS[arg]) return { kind: "focus", toward: DIRECTIONS[arg] };
      return `goto_split:${arg}: previous, next, left, right, up or down`;
    case "resize_split": {
      const dir = DIRECTIONS[arg.split(",")[0].trim()];
      return dir ? { kind: "resize", toward: dir } : `resize_split:${arg}: left, right, up or down`;
    }
    case "equalize_splits":
      return { kind: "equalize" };
    case "toggle_split_zoom":
      return { kind: "zoom" };
    case "previous_tab":
      return { kind: "tabCycle", step: -1 };
    case "next_tab":
      return { kind: "tabCycle", step: 1 };
    case "last_tab":
      return { kind: "goToTab", index: -1 };
    case "goto_tab": {
      const n = Number(arg);
      return Number.isInteger(n) && n >= 1 ? { kind: "goToTab", index: n - 1 } : `goto_tab:${arg}: a tab number from 1`;
    }
    case "increase_font_size":
      return { kind: "fontSize", step: 1 };
    case "decrease_font_size":
      return { kind: "fontSize", step: -1 };
    case "reset_font_size":
      return { kind: "fontSize", step: 0 };
    case "start_search":
      return { kind: "find" };
    case "navigate_search":
      if (arg === "next" || arg === "previous") return { kind: "findStep", older: arg === "next" };
      return `navigate_search:${arg}: next or previous`;
    case "toggle_sidebar":
      return { kind: "sidebar" };
    case "show_shortcuts":
      return { kind: "shortcuts" };
    case "open_theme_picker":
      return { kind: "themes" };
    case "open_settings":
      return { kind: "settings" };
    default:
      return `unknown action "${a}"`;
  }
}

/** The shortcuts after `lines` (the config's `keybind` values, in order)
 * change `defaults`, and a message for each line that couldn't be used. */
export function applyKeybinds(defaults: Binding[], lines: string[]): { table: Binding[]; errors: string[] } {
  let table = defaults.map((b) => ({ ...b, mods: [...b.mods].sort().join("") }));
  const errors: string[] = [];
  for (const line of lines) {
    const bad = (why: string) => errors.push(`keybind = ${line}: ${why}`);
    if (line.trim() === "clear") {
      table = [];
      continue;
    }
    // The trigger may end in "=" (super+=): the action starts after the
    // last "=" that has something after it.
    const at = line.lastIndexOf("=", line.length - 2);
    if (at <= 0) {
      bad("expected trigger=action");
      continue;
    }
    const trigger = parseTrigger(line.slice(0, at).trim());
    if (typeof trigger === "string") {
      bad(trigger);
      continue;
    }
    const action = parseAction(line.slice(at + 1).trim());
    if (typeof action === "string" && action !== "unbind") {
      bad(action);
      continue;
    }
    table = table.filter((b) => !(b.code === trigger.code && b.mods === trigger.mods));
    if (action !== "unbind") table.push({ ...trigger, action });
  }
  return { table, errors };
}
