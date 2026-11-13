// Workspace shortcuts (tabs and splits), Ghostty's defaults for each platform.
// They are taken before the terminal sees the key.

import type { KeyLike, Platform } from "../input/keyboard";
import type { Dir, Direction } from "./layout";

export type Action =
  | { kind: "newTab" }
  | { kind: "close" }
  | { kind: "split"; dir: Dir }
  | { kind: "focusCycle"; step: 1 | -1 }
  | { kind: "focus"; toward: Direction }
  | { kind: "resize"; toward: Direction }
  | { kind: "equalize" }
  | { kind: "zoom" }
  | { kind: "tabCycle"; step: 1 | -1 }
  /** 0-based; -1 is the last tab. */
  | { kind: "goToTab"; index: number }
  | { kind: "sidebar" }
  | { kind: "shortcuts" }
  /** Bigger (1), smaller (-1), or back to the config's size (0). */
  | { kind: "fontSize"; step: 1 | -1 | 0 }
  /** The theme picker (no key; View → Theme…). */
  | { kind: "themes" };

/** Modifiers as a string of letters: c(trl) a(lt) s(hift) m(eta: Cmd/Super). */
type Mods = string;

export interface Binding {
  mods: Mods;
  code: string;
  action: Action;
}

const ARROWS: Array<[string, Direction]> = [
  ["ArrowLeft", "left"],
  ["ArrowRight", "right"],
  ["ArrowUp", "up"],
  ["ArrowDown", "down"],
];

const DIGITS = ["1", "2", "3", "4", "5", "6", "7", "8", "9"].map((d) => `Digit${d}`);

function table(p: Platform): Binding[] {
  const b: Binding[] = [];
  const add = (mods: Mods, code: string, action: Action) => b.push({ mods, code, action });
  if (p === "mac") {
    add("m", "KeyT", { kind: "newTab" });
    add("m", "KeyW", { kind: "close" });
    add("m", "KeyD", { kind: "split", dir: "row" });
    add("ms", "KeyD", { kind: "split", dir: "column" });
    add("m", "BracketLeft", { kind: "focusCycle", step: -1 });
    add("m", "BracketRight", { kind: "focusCycle", step: 1 });
    for (const [code, toward] of ARROWS) {
      add("am", code, { kind: "focus", toward });
      add("cm", code, { kind: "resize", toward });
    }
    add("cm", "Equal", { kind: "equalize" });
    add("ms", "Enter", { kind: "zoom" });
    add("ms", "BracketLeft", { kind: "tabCycle", step: -1 });
    add("ms", "BracketRight", { kind: "tabCycle", step: 1 });
    DIGITS.forEach((code, i) => add("m", code, { kind: "goToTab", index: i === 8 ? -1 : i }));
    add("ms", "KeyB", { kind: "sidebar" });
    add("m", "Slash", { kind: "shortcuts" });
    add("m", "Equal", { kind: "fontSize", step: 1 });
    add("ms", "Equal", { kind: "fontSize", step: 1 }); // Cmd+Plus
    add("m", "Minus", { kind: "fontSize", step: -1 });
    add("m", "Digit0", { kind: "fontSize", step: 0 });
  } else {
    add("cs", "KeyT", { kind: "newTab" });
    add("cs", "KeyW", { kind: "close" });
    add("cs", "KeyO", { kind: "split", dir: "row" });
    add("cs", "KeyE", { kind: "split", dir: "column" });
    add("cm", "BracketLeft", { kind: "focusCycle", step: -1 });
    add("cm", "BracketRight", { kind: "focusCycle", step: 1 });
    for (const [code, toward] of ARROWS) {
      add("ca", code, { kind: "focus", toward });
      add("cms", code, { kind: "resize", toward });
    }
    add("cms", "Equal", { kind: "equalize" });
    add("cs", "Enter", { kind: "zoom" });
    add("c", "PageUp", { kind: "tabCycle", step: -1 });
    add("c", "PageDown", { kind: "tabCycle", step: 1 });
    DIGITS.forEach((code, i) => add("a", code, { kind: "goToTab", index: i === 8 ? -1 : i }));
    add("cs", "KeyB", { kind: "sidebar" });
    // Not Ctrl+/: shells and editors take it (undo).
    add("cs", "Slash", { kind: "shortcuts" });
    // Ghostty's: Ctrl+= (and Ctrl+Plus), Ctrl+-, Ctrl+0.
    add("c", "Equal", { kind: "fontSize", step: 1 });
    add("cs", "Equal", { kind: "fontSize", step: 1 });
    add("c", "Minus", { kind: "fontSize", step: -1 });
    add("c", "Digit0", { kind: "fontSize", step: 0 });
  }
  add("c", "Tab", { kind: "tabCycle", step: 1 });
  add("cs", "Tab", { kind: "tabCycle", step: -1 });
  return b;
}

const tables = new Map<Platform, Binding[]>();

/** Every shortcut on `platform`, in the order they're defined. */
export function bindings(platform: Platform): Binding[] {
  return table(platform);
}

function modsOf(e: KeyLike): Mods {
  return (e.ctrlKey ? "c" : "") + (e.altKey ? "a" : "") + (e.metaKey ? "m" : "") + (e.shiftKey ? "s" : "");
}

/** Sorted letters, so "ms" and "sm" compare equal. */
const norm = (m: Mods) => [...m].sort().join("");

/** The workspace action for a keydown, if it is one of the shortcuts. */
export function actionFor(e: KeyLike, platform: Platform): Action | null {
  if (e.isComposing) return null;
  let t = tables.get(platform);
  if (!t) {
    t = table(platform).map((b) => ({ ...b, mods: norm(b.mods) }));
    tables.set(platform, t);
  }
  const m = norm(modsOf(e));
  return t.find((b) => b.code === e.code && b.mods === m)?.action ?? null;
}
