// The keyboard shortcut sheet (Cmd+/, Help → Keyboard Shortcuts): every
// shortcut, grouped. The workspace's keys come from the keymap itself, so the
// sheet can't fall out of date; the terminal's own keys are listed here.

import type { Platform } from "../input/keyboard";
import { bindings, type Action, type Binding } from "./keymap";
import "../ui.css";
import "./shortcuts.css";

export interface SheetRow {
  label: string;
  /** Alternatives, each shown as one key chip. */
  keys: string[];
}

export interface SheetGroup {
  title: string;
  rows: SheetRow[];
}

const MAC_MODS: Array<[string, string]> = [
  ["c", "⌃"],
  ["a", "⌥"],
  ["s", "⇧"],
  ["m", "⌘"],
];
const PC_MODS: Array<[string, string]> = [
  ["c", "Ctrl"],
  ["a", "Alt"],
  ["s", "Shift"],
  ["m", "Super"],
];

const KEY_NAMES: Record<string, string> = {
  BracketLeft: "[",
  BracketRight: "]",
  Comma: ",",
  Equal: "=",
  Minus: "−",
  Slash: "/",
  ArrowLeft: "←",
  ArrowRight: "→",
  ArrowUp: "↑",
  ArrowDown: "↓",
  PageUp: "PgUp",
  PageDown: "PgDn",
};

export function keyName(code: string): string {
  return KEY_NAMES[code] ?? code.replace(/^(Key|Digit)/, "");
}

function modsText(mods: string, platform: Platform): string {
  if (platform === "mac") return MAC_MODS.filter(([m]) => mods.includes(m)).map(([, t]) => t).join("");
  return PC_MODS.filter(([m]) => mods.includes(m)).map(([, t]) => `${t}+`).join("");
}

/** One key combination as shown: `⇧⌘B`, `Ctrl+Shift+B`. */
export function keyText(mods: string, key: string, platform: Platform): string {
  return modsText(mods, platform) + key;
}

const same = (a: Action, b: Action) => JSON.stringify(a) === JSON.stringify(b);
const isArrow = (code: string) => code.startsWith("Arrow");
const isDigit = (code: string) => code.startsWith("Digit");

/**
 * The keys for a row doing `actions` (one per direction or number): the nth
 * binding of each action makes the nth alternative. Arrows and digits under
 * the same modifiers collapse to `⌥⌘ Arrows` and `⌘1–8`.
 */
function keysFor(actions: Action[], table: Binding[], platform: Platform): string[] {
  const per = actions.map((a) => table.filter((b) => same(b.action, a)));
  const n = Math.max(0, ...per.map((bs) => bs.length));
  const out: string[] = [];
  for (let i = 0; i < n; i++) {
    const bs = per.map((list) => list[i]).filter((b): b is Binding => !!b);
    if (bs.length === 0) continue;
    const mods = bs[0].mods;
    const oneMods = bs.every((b) => b.mods === mods);
    if (bs.length > 1 && oneMods && bs.every((b) => isArrow(b.code))) {
      out.push(keyText(mods, platform === "mac" ? " Arrows" : "Arrows", platform));
    } else if (bs.length > 2 && oneMods && bs.every((b) => isDigit(b.code))) {
      out.push(keyText(mods, `${keyName(bs[0].code)}–${keyName(bs[bs.length - 1].code)}`, platform));
    } else {
      out.push(bs.map((b) => keyText(b.mods, keyName(b.code), platform)).join(" / "));
    }
  }
  return out;
}

const DIRS = ["left", "right", "up", "down"] as const;

/** Every shortcut on `platform`, grouped. */
export function sheetFor(platform: Platform): SheetGroup[] {
  const table = bindings(platform);
  const row = (label: string, ...actions: Action[]): SheetRow => ({ label, keys: keysFor(actions, table, platform) });
  const mac = platform === "mac";
  const k = (mods: string, key: string) => keyText(mods, key, platform);
  const groups: SheetGroup[] = [
    {
      title: "Tabs",
      rows: [
        row("New tab", { kind: "newTab" }),
        row("Previous / next tab", { kind: "tabCycle", step: -1 }, { kind: "tabCycle", step: 1 }),
        row(
          "Tab 1–8",
          ...[0, 1, 2, 3, 4, 5, 6, 7].map((index): Action => ({ kind: "goToTab", index })),
        ),
        row("Last tab", { kind: "goToTab", index: -1 }),
      ],
    },
    {
      title: "Panes",
      rows: [
        row("Split right / down", { kind: "split", dir: "row" }, { kind: "split", dir: "column" }),
        row("Close pane", { kind: "close" }),
        row("Previous / next pane", { kind: "focusCycle", step: -1 }, { kind: "focusCycle", step: 1 }),
        row("Pane in a direction", ...DIRS.map((toward): Action => ({ kind: "focus", toward }))),
        row("Resize pane", ...DIRS.map((toward): Action => ({ kind: "resize", toward }))),
        row("Equalize panes", { kind: "equalize" }),
        row("Zoom pane", { kind: "zoom" }),
      ],
    },
    {
      title: "Window",
      rows: [
        row("Sidebar", { kind: "sidebar" }),
        row("Keyboard shortcuts", { kind: "shortcuts" }),
        row("Settings", { kind: "settings" }),
        row("Bigger / smaller text", { kind: "fontSize", step: 1 }, { kind: "fontSize", step: -1 }),
        row("Default text size", { kind: "fontSize", step: 0 }),
      ],
    },
    {
      title: "Find",
      rows: [
        row("Find", { kind: "find" }),
        row("Next / previous match", { kind: "findStep", older: true }, { kind: "findStep", older: false }),
      ],
    },
    {
      title: "Terminal",
      rows: [
        { label: "Copy / paste", keys: [mac ? `${k("m", "C")} / ${k("m", "V")}` : `${k("cs", "C")} / ${k("cs", "V")}`] },
        { label: "Scroll a page", keys: [`${k("s", "PgUp")} / ${k("s", "PgDn")}`] },
        { label: "Top / bottom of scrollback", keys: [mac ? `${k("m", "Home")} / ${k("m", "End")}` : `${k("s", "Home")} / ${k("s", "End")}`] },
        { label: "Open a link", keys: [mac ? "⌘-click" : "Ctrl+click"] },
        { label: "Select a rectangle", keys: [mac ? "⌥-drag" : "Alt+drag"] },
        { label: "Select in mouse programs", keys: [mac ? "⇧-drag" : "Shift+drag"] },
        ...(mac
          ? [
              { label: "Start / end of line", keys: [`${k("m", "←")} / ${k("m", "→")}`] },
              { label: "Word left / right", keys: [`${k("a", "←")} / ${k("a", "→")}`] },
              { label: "Delete the line", keys: [k("m", "⌫")] },
            ]
          : []),
      ],
    },
  ];
  return groups.map((g) => ({ ...g, rows: g.rows.filter((r) => r.keys.length > 0) }));
}

/** Every shortcut on `platform` in columns of groups: the sheet's body,
 * and Settings → Shortcuts. */
export function sheetGrid(platform: Platform): HTMLDivElement {
  const cols = document.createElement("div");
  cols.className = "shortcut-grid";
  for (const g of sheetFor(platform)) {
    const box = document.createElement("div");
    box.className = "shortcut-group";
    const h = document.createElement("div");
    h.className = "shortcut-group-title";
    h.textContent = g.title;
    box.append(h);
    for (const r of g.rows) {
      const line = document.createElement("div");
      line.className = "shortcut-row";
      const label = document.createElement("span");
      label.className = "shortcut-label";
      label.textContent = r.label;
      line.append(label);
      for (const key of r.keys) {
        const kbd = document.createElement("kbd");
        kbd.className = "shortcut-key";
        kbd.textContent = key;
        line.append(kbd);
      }
      box.append(line);
    }
    cols.append(box);
  }
  return cols;
}

/** The sheet itself: an overlay over the workspace. */
export class ShortcutSheet {
  readonly el: HTMLDivElement;
  visible = false;

  constructor(private readonly platform: Platform) {
    this.el = document.createElement("div");
    this.el.className = "shortcut-sheet";
    this.el.style.display = "none";
    // Any click closes it, and keeps focus in the terminal.
    this.el.addEventListener("mousedown", (e) => {
      e.preventDefault();
      this.hide();
    });
    this.rebuild();
  }

  /** Fill it from the shortcuts in use (again, after the config changed them). */
  rebuild(): void {
    const platform = this.platform;
    const panel = document.createElement("div");
    panel.className = "shortcut-panel";
    const head = document.createElement("div");
    head.className = "shortcut-head";
    const title = document.createElement("div");
    title.className = "shortcut-title";
    title.textContent = "Keyboard Shortcuts";
    const hint = document.createElement("div");
    hint.className = "shortcut-hint";
    hint.textContent = "Esc to close";
    head.append(title, hint);
    const cols = sheetGrid(platform);
    panel.append(head, cols);
    this.el.replaceChildren(panel);
  }

  toggle(): void {
    if (this.visible) this.hide();
    else this.show();
  }

  show(): void {
    this.visible = true;
    this.el.style.display = "flex";
  }

  hide(): void {
    this.visible = false;
    this.el.style.display = "none";
  }
}
