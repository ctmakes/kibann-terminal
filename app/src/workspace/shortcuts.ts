// The keyboard shortcut sheet (Cmd+/, Help → Keyboard Shortcuts): every
// shortcut, grouped. The workspace's keys come from the keymap itself, so the
// sheet can't fall out of date; the terminal's own keys are listed here.

import type { Platform } from "../input/keyboard";
import { bindings, type Action, type Binding } from "./keymap";

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
  Equal: "=",
  Slash: "/",
  ArrowLeft: "←",
  ArrowRight: "→",
  ArrowUp: "↑",
  ArrowDown: "↓",
  PageUp: "PgUp",
  PageDown: "PgDn",
};

function keyName(code: string): string {
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
      rows: [row("Sidebar", { kind: "sidebar" }), row("Keyboard shortcuts", { kind: "shortcuts" })],
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

/** The sheet itself: an overlay over the workspace. */
export class ShortcutSheet {
  readonly el: HTMLDivElement;
  visible = false;

  constructor(platform: Platform) {
    this.el = document.createElement("div");
    this.el.className = "shortcut-sheet";
    Object.assign(this.el.style, {
      position: "absolute",
      inset: "0",
      zIndex: "30",
      display: "none",
      alignItems: "center",
      justifyContent: "center",
      background: "var(--k-scrim)",
      font: "12px -apple-system, system-ui, sans-serif",
      color: "var(--k-strong)",
    });
    // Any click closes it, and keeps focus in the terminal.
    this.el.addEventListener("mousedown", (e) => {
      e.preventDefault();
      this.hide();
    });
    const panel = document.createElement("div");
    Object.assign(panel.style, {
      background: "var(--k-panel)",
      border: "1px solid var(--k-divider)",
      borderRadius: "10px",
      boxShadow: "0 12px 40px var(--k-shadow)",
      padding: "16px 20px 18px",
      width: "min(760px, calc(100% - 32px))",
      maxHeight: "calc(100% - 32px)",
      overflowY: "auto",
      boxSizing: "border-box",
    });
    const head = document.createElement("div");
    Object.assign(head.style, { display: "flex", alignItems: "baseline", marginBottom: "10px" });
    const title = document.createElement("div");
    title.textContent = "Keyboard Shortcuts";
    Object.assign(title.style, { flex: "1", fontSize: "14px", fontWeight: "600", color: "var(--k-bright)" });
    const hint = document.createElement("div");
    hint.textContent = "Esc to close";
    hint.style.color = "var(--k-muted)";
    head.append(title, hint);
    const cols = document.createElement("div");
    Object.assign(cols.style, {
      display: "grid",
      gridTemplateColumns: "repeat(auto-fit, minmax(290px, 1fr))",
      columnGap: "28px",
      rowGap: "12px",
    });
    for (const g of sheetFor(platform)) {
      const box = document.createElement("div");
      box.className = "shortcut-group";
      const h = document.createElement("div");
      h.textContent = g.title;
      Object.assign(h.style, { color: "var(--k-muted)", fontSize: "11px", fontWeight: "600", margin: "4px 0 4px" });
      box.append(h);
      for (const r of g.rows) {
        const line = document.createElement("div");
        line.className = "shortcut-row";
        Object.assign(line.style, { display: "flex", alignItems: "center", gap: "8px", padding: "3px 0" });
        const label = document.createElement("span");
        label.textContent = r.label;
        label.style.flex = "1";
        line.append(label);
        for (const key of r.keys) {
          const kbd = document.createElement("kbd");
          kbd.textContent = key;
          Object.assign(kbd.style, {
            font: "11px -apple-system, system-ui, sans-serif",
            letterSpacing: "0.5px",
            color: "var(--k-bright)",
            background: "var(--k-kbd)",
            border: "1px solid var(--k-divider)",
            borderRadius: "4px",
            padding: "1px 6px",
            whiteSpace: "nowrap",
          });
          line.append(kbd);
        }
        box.append(line);
      }
      cols.append(box);
    }
    panel.append(head, cols);
    this.el.append(panel);
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
