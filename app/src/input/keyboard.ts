import { Mods, type InputEvent } from "../proto";
import { graphemes } from "../wcwidth";

export type Platform = "mac" | "other";

export function detectPlatform(): Platform {
  const p = typeof navigator !== "undefined" ? navigator.platform || navigator.userAgent : "";
  return /Mac|iPhone|iPad/i.test(p) ? "mac" : "other";
}

/** The subset of KeyboardEvent the mapping needs, so it can be unit tested. */
export interface KeyLike {
  key: string;
  code: string;
  repeat: boolean;
  shiftKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  metaKey: boolean;
  isComposing: boolean;
  keyCode: number;
  getModifierState?(key: string): boolean;
}

/** Which side of each modifier is held. Browsers only say "shift is down". */
export class ModSides {
  shift = false;
  ctrl = false;
  alt = false;
  super = false;

  /** Feed every keydown/keyup so a right-hand modifier is remembered. */
  update(code: string, down: boolean): void {
    const set = (field: "shift" | "ctrl" | "alt" | "super", right: boolean) => {
      if (down) this[field] = right;
      else if (right) this[field] = false;
    };
    switch (code) {
      case "ShiftLeft": return set("shift", false);
      case "ShiftRight": return set("shift", true);
      case "ControlLeft": return set("ctrl", false);
      case "ControlRight": return set("ctrl", true);
      case "AltLeft": return set("alt", false);
      case "AltRight": return set("alt", true);
      case "MetaLeft": case "OSLeft": return set("super", false);
      case "MetaRight": case "OSRight": return set("super", true);
    }
  }

  clear(): void {
    this.shift = this.ctrl = this.alt = this.super = false;
  }
}

export function modsOf(
  e: Pick<KeyLike, "shiftKey" | "ctrlKey" | "altKey" | "metaKey" | "getModifierState">,
  sides?: ModSides,
): number {
  let m = 0;
  if (e.shiftKey) m |= Mods.SHIFT | (sides?.shift ? Mods.SHIFT_RIGHT : 0);
  if (e.ctrlKey) m |= Mods.CTRL | (sides?.ctrl ? Mods.CTRL_RIGHT : 0);
  if (e.altKey) m |= Mods.ALT | (sides?.alt ? Mods.ALT_RIGHT : 0);
  if (e.metaKey) m |= Mods.SUPER | (sides?.super ? Mods.SUPER_RIGHT : 0);
  if (e.getModifierState?.("CapsLock")) m |= Mods.CAPS_LOCK;
  if (e.getModifierState?.("NumLock")) m |= Mods.NUM_LOCK;
  return m;
}

/** Shortcuts the webview or app handles instead of the terminal. */
export function isPassthrough(e: KeyLike, platform: Platform): boolean {
  if (platform === "mac") {
    if (!e.metaKey || e.ctrlKey || e.altKey) return false;
    return ["KeyQ", "KeyW", "KeyH", "KeyM", "KeyV", "KeyC"].includes(e.code) && !e.shiftKey;
  }
  if (e.ctrlKey && e.shiftKey && !e.altKey && (e.code === "KeyV" || e.code === "KeyC")) return true;
  if (e.shiftKey && !e.ctrlKey && !e.altKey && e.code === "Insert") return true;
  return false;
}

export type KeyDecision = "ime" | "passthrough" | InputEvent;

/** What to do with a keydown. */
export function decideKeyDown(e: KeyLike, sides: ModSides, platform: Platform): KeyDecision {
  if (e.isComposing || e.keyCode === 229 || e.key === "Dead" || e.key === "Process") return "ime";
  if (isPassthrough(e, platform)) return "passthrough";
  const printable = e.key !== "Unidentified" && graphemes(e.key).length === 1;
  const text = printable && !e.ctrlKey && !e.metaKey ? e.key : null;
  return {
    kind: "key",
    action: e.repeat ? "repeat" : "press",
    code: e.code,
    key: e.key,
    text,
    mods: modsOf(e, sides),
    composing: false,
  };
}

export function keyUpEvent(e: KeyLike, sides: ModSides): InputEvent {
  return { kind: "key", action: "release", code: e.code, key: e.key, text: null, mods: modsOf(e, sides), composing: false };
}

export interface KeyboardSink {
  send(e: InputEvent): void;
  /** Called on any keyboard activity (restarts cursor blink). */
  activity(): void;
}

/** Listens on the hidden textarea and turns keys into input events. */
export class KeyboardInput {
  readonly sides = new ModSides();
  private pressed = new Set<string>();
  private readonly offs: Array<() => void> = [];

  constructor(
    readonly target: HTMLTextAreaElement,
    private readonly sink: KeyboardSink,
    private readonly platform: Platform = detectPlatform(),
  ) {
    const on = <K extends keyof HTMLElementEventMap>(type: K, fn: (e: HTMLElementEventMap[K]) => void) => {
      target.addEventListener(type, fn);
      this.offs.push(() => target.removeEventListener(type, fn));
    };
    on("keydown", (e) => this.onKeyDown(e));
    on("keyup", (e) => this.onKeyUp(e));
    on("blur", () => {
      this.sides.clear();
      this.pressed.clear();
    });
  }

  private onKeyDown(e: KeyboardEvent): void {
    this.sides.update(e.code, true);
    const d = decideKeyDown(e, this.sides, this.platform);
    if (d === "ime" || d === "passthrough") return;
    e.preventDefault();
    this.pressed.add(e.code);
    this.sink.send(d);
    this.sink.activity();
  }

  private onKeyUp(e: KeyboardEvent): void {
    this.sides.update(e.code, false);
    if (!this.pressed.delete(e.code)) return;
    e.preventDefault();
    this.sink.send(keyUpEvent(e, this.sides));
  }

  dispose(): void {
    for (const off of this.offs) off();
  }
}
