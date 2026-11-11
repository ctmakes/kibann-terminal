import { Flag, Underline, withUnderline, type CursorStyle } from "../../proto";
import { Screen, style, DEFAULT_FG } from "../screen";
import { BASE16, contrast, hsl, rng, xterm256 } from "./palette";

export type Params = Record<string, string>;

export interface Fixture {
  name: string;
  description: string;
  build(cols: number, rows: number, tick: number, params: Params): Screen;
  interval?: number;
}

const dim = style(0x8a8a8a);
const heading = style(0xffffff, null, Flag.BOLD);

function title(s: Screen, text: string): void {
  s.write(0, 0, text, heading);
  s.cursor = { ...s.cursor, in_viewport: false };
}

const palette: Fixture = {
  name: "palette",
  description: "16 and 256 color charts as fg and bg, truecolor gradients",
  build(cols, rows) {
    const s = new Screen(cols, rows);
    title(s, "16 colors (bg, then fg)");
    for (let i = 0; i < 16; i++) {
      const x = (i % 8) * 5;
      const y = 1 + Math.floor(i / 8);
      s.write(x, y, ` ${String(i).padStart(2)} `, style(contrast(BASE16[i]), BASE16[i]));
    }
    for (let i = 0; i < 16; i++) s.write(42 + (i % 8) * 4, 1 + Math.floor(i / 8), "Aa ", style(BASE16[i], null, i >= 8 ? Flag.BOLD : 0));
    s.write(0, 4, "256 colors", heading);
    for (let r = 0; r < 6; r++) {
      for (let n = 0; n < 36; n++) {
        const i = 16 + r * 36 + n;
        s.write(n * 2, 5 + r, "  ", style(DEFAULT_FG, xterm256(i)));
      }
    }
    for (let i = 232; i < 256; i++) s.write((i - 232) * 3, 11, "   ", style(DEFAULT_FG, xterm256(i)));
    s.write(0, 13, "truecolor", heading);
    for (let x = 0; x < cols; x++) {
      s.write(x, 14, " ", style(DEFAULT_FG, hsl((x / cols) * 360, 0.85, 0.5)));
      s.write(x, 15, "▀", style(hsl((x / cols) * 360, 0.9, 0.7), hsl((x / cols) * 360, 0.9, 0.3)));
      const g = Math.round((x / Math.max(1, cols - 1)) * 255);
      s.write(x, 16, " ", style(DEFAULT_FG, (g << 16) | (g << 8) | g));
    }
    const line = "The quick brown fox jumps over the lazy dog 0123456789";
    for (let i = 0; i < 6 && 18 + i < rows; i++) {
      s.write(0, 18 + i, line, style(hsl(i * 60, 0.7, 0.65), i % 2 ? 0x202833 : null));
    }
    return s;
  },
};

const styles: Fixture = {
  name: "styles",
  description: "every attribute alone and combined, all underline styles",
  build(cols, rows) {
    const s = new Screen(cols, rows);
    title(s, "Text attributes");
    const lines: Array<[string, number, number | null, number | null]> = [
      ["plain", 0, null, null],
      ["bold", Flag.BOLD, null, null],
      ["italic", Flag.ITALIC, null, null],
      ["bold italic", Flag.BOLD | Flag.ITALIC, null, null],
      ["faint", Flag.FAINT, null, null],
      ["strikethrough", Flag.STRIKE, null, null],
      ["overline", Flag.OVERLINE, null, null],
      ["underline single", withUnderline(0, Underline.SINGLE), null, null],
      ["underline double", withUnderline(0, Underline.DOUBLE), null, null],
      ["underline curly", withUnderline(0, Underline.CURLY), null, null],
      ["underline dotted", withUnderline(0, Underline.DOTTED), null, null],
      ["underline dashed", withUnderline(0, Underline.DASHED), null, null],
      ["curly, red underline", withUnderline(0, Underline.CURLY), null, 0xff5555],
      ["single, blue underline", withUnderline(0, Underline.SINGLE), null, 0x5599ff],
      ["faint on color", Flag.FAINT, 0x2d4f2d, null],
      ["bold+italic+underline+strike", Flag.BOLD | Flag.ITALIC | Flag.STRIKE | withUnderline(0, Underline.SINGLE), null, null],
    ];
    lines.forEach(([label, flags, bg, ul], i) => {
      s.write(0, 2 + i, label.padEnd(30), dim);
      s.write(30, 2 + i, "Sphinx of black quartz, judge my vow", style(DEFAULT_FG, bg, flags, ul));
    });
    const y = 3 + lines.length;
    s.write(0, y, "invisible".padEnd(30), dim);
    s.write(30, y, "[", style());
    s.write(31, y, "hidden", style(DEFAULT_FG, null, Flag.INVISIBLE));
    s.write(37, y, "]", style());
    s.write(0, y + 1, "inverse (resolved by backend)".padEnd(30), dim);
    s.write(30, y + 1, " swapped colors ", style(0x181818, DEFAULT_FG));
    s.write(0, y + 2, "underlined run across styles".padEnd(30), dim);
    let x = 30;
    for (const [fg, flags] of [[0xff8888, 0], [0x88ff88, Flag.BOLD], [0x8888ff, Flag.ITALIC]] as const) {
      x = s.write(x, y + 2, "curly ", style(fg, null, flags | withUnderline(0, Underline.CURLY)));
    }
    return s;
  },
};

function table(s: Screen, x: number, y: number, chars: string, st = style()): void {
  // chars: 11 glyphs: ┌ ┬ ┐ ├ ┼ ┤ └ ┴ ┘ ─ │
  const [tl, tm, tr, ml, mm, mr, bl, bm, br, h, v] = [...chars];
  const w = 6;
  const rowsOut = [
    tl + h.repeat(w) + tm + h.repeat(w) + tr,
    v + " cell ".padEnd(w) + v + " cell ".padEnd(w) + v,
    ml + h.repeat(w) + mm + h.repeat(w) + mr,
    v + " 日本 ".padEnd(w - 2) + v + " ok ".padEnd(w) + v,
    bl + h.repeat(w) + bm + h.repeat(w) + br,
  ];
  rowsOut.forEach((r, i) => s.write(x, y + i, r, st));
}

const boxdraw: Fixture = {
  name: "boxdraw",
  description: "every shape-drawn glyph, nested tables in each line style",
  build(cols, rows) {
    const s = new Screen(cols, rows);
    title(s, "Box drawing U+2500–257F, blocks U+2580–259F, braille, Powerline");
    for (let r = 0; r < 8; r++) {
      const base = 0x2500 + r * 16;
      s.write(0, 1 + r, base.toString(16).toUpperCase(), dim);
      for (let i = 0; i < 16; i++) s.write(6 + i * 2, 1 + r, String.fromCodePoint(base + i));
      // The same glyphs touching, to check joins.
      for (let i = 0; i < 16; i++) s.write(40 + i, 1 + r, String.fromCodePoint(base + i), style(0x9ecbff));
    }
    for (let r = 0; r < 2; r++) {
      const base = 0x2580 + r * 16;
      s.write(0, 10 + r, base.toString(16).toUpperCase(), dim);
      for (let i = 0; i < 16; i++) s.write(6 + i * 2, 10 + r, String.fromCodePoint(base + i), style(0xffcc66));
      for (let i = 0; i < 16; i++) s.write(40 + i, 10 + r, String.fromCodePoint(base + i), style(0xffcc66));
    }
    s.write(0, 13, "2800", dim);
    for (let i = 0; i < 64; i++) s.write(6 + i, 13, String.fromCodePoint(0x2800 + i * 4 + (i % 4)), style(0x99ee99));
    s.write(0, 14, "28C0", dim);
    for (let i = 0; i < 64; i++) s.write(6 + i, 14, String.fromCodePoint(0x28c0 + i), style(0x99ee99));
    s.write(0, 16, "E0B0", dim);
    s.write(6, 16, " main ", style(0x181818, 0x88c0d0));
    s.write(12, 16, "", style(0x88c0d0, 0x4c566a));
    s.write(13, 16, " src/app ", style(0xeceff4, 0x4c566a));
    s.write(22, 16, "", style(0x4c566a));
    s.write(24, 16, " thin ", style(0x88c0d0));
    s.write(36, 16, "", style(0xa3be8c));
    s.write(37, 16, " 12:00 ", style(0x181818, 0xa3be8c));
    table(s, 0, 18, "┌┬┐├┼┤└┴┘─│");
    table(s, 16, 18, "╔╦╗╠╬╣╚╩╝═║", style(0xffcc66));
    table(s, 32, 18, "┏┳┓┣╋┫┗┻┛━┃", style(0xff8888));
    table(s, 48, 18, "╭┬╮├┼┤╰┴╯─│", style(0x99ee99));
    table(s, 64, 18, "╒╤╕╞╪╡╘╧╛═│", style(0xc397d8));
    table(s, 80, 18, "╓╥╖╟╫╢╙╨╜─║", style(0x70c0b1));
    s.write(0, 24, "┄┄┄┅┅┅ ┈┈┈┉┉┉ ╌╌╌╍╍╍ ┆┇┊┋╎╏ ╱╲╳ ╴╵╶╷╸╹╺╻╼╽╾╿", style(0x9ecbff));
    return s;
  },
};

/** White on black so pixel tests can check strokes are continuous. */
const boxjoin: Fixture = {
  name: "boxjoin",
  description: "long runs of lines and blocks for pixel continuity tests",
  build(cols, rows) {
    const s = new Screen(cols, rows, 0xffffff, 0x000000);
    const st = style(0xffffff);
    s.write(0, 0, "─".repeat(cols), st);
    s.write(0, 2, "━".repeat(cols), st);
    s.write(0, 4, "═".repeat(cols), st);
    s.write(0, 6, "█".repeat(cols), st);
    for (let y = 8; y < rows; y++) {
      s.write(0, y, "│", st);
      s.write(2, y, "┃", st);
      s.write(4, y, "║", st);
      s.write(6, y, "█", st);
      s.write(8, y, "┼".repeat(Math.max(0, Math.min(10, cols - 8))), st);
      s.write(20, y, "╬".repeat(Math.max(0, Math.min(10, cols - 20))), st);
    }
    s.cursor = { ...s.cursor, in_viewport: false };
    return s;
  },
};

const unicode: Fixture = {
  name: "unicode",
  description: "CJK, emoji, combining marks, wide char at the last column",
  build(cols, rows) {
    const s = new Screen(cols, rows);
    title(s, "Unicode");
    const lines = [
      "CJK:       日本語のテキスト 中文字符 한국어 텍스트",
      "Mixed:     abc日本def中文ghi",
      "Emoji:     😀 👍🏽 👨‍👩‍👧 🇯🇵 ❤️ 🚀 ✅",
      "Combining: é ñ ā̈ Z̵̲",
      "Symbols:   ✓ ✗ → ← ★ ⚡ ⏎ λ Ω ≠ ≤ ∞ ©",
      "Greek:     Αλφάβητο  Cyrillic: Кириллица",
      "Fullwidth: ＡＢＣ　１２３　！？",
    ];
    lines.forEach((l, i) => s.write(0, 2 + i, l));
    const y = 3 + lines.length;
    s.write(0, y, "Wide char at the last column wraps (spacer head):", dim);
    s.write(0, y + 1, "x".repeat(cols - 1), dim);
    s.write(cols - 1, y + 1, "日");
    s.write(0, y + 2, "日", style(0xffcc66));
    s.write(0, y + 3, "Wide chars on colored backgrounds:", dim);
    s.write(0, y + 4, "日本", style(0x181818, 0xffcc66));
    s.write(4, y + 4, "語", style(0x181818, 0x88c0d0));
    s.write(7, y + 4, "😀", style(DEFAULT_FG, 0x5a3d6e));
    return s;
  },
};

const cursor: Fixture = {
  name: "cursor",
  description: "cursor styles (?style=bar|block|underline|block_hollow, ?wide=1, ?color=hex)",
  build(cols, rows, _tick, p) {
    const s = new Screen(cols, rows);
    const st = (p.style as CursorStyle) || "block";
    s.write(0, 0, `Cursor: ${st}${p.wide === "1" ? " on wide char" : ""}`, heading);
    s.write(0, 2, "$ echo hello 日本 world", style());
    s.write(2, 4, "┌──────┐", style(0x9ecbff));
    s.cursor = {
      x: p.wide === "1" ? 13 : p.x ? Number(p.x) : 7,
      y: 2,
      in_viewport: true,
      visible: p.visible !== "0",
      blinking: p.blink === "1",
      style: st,
      wide: p.wide === "1",
      color: p.color ? parseInt(p.color, 16) : null,
    };
    return s;
  },
};

const SPIN = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";

const partial: Fixture = {
  name: "partial",
  description: "animated: spinner, counter and a scrolling region; only those rows change",
  interval: 100,
  build(cols, rows, tick) {
    const s = new Screen(cols, rows);
    title(s, "Partial updates (static rows should never repaint)");
    s.write(0, 2, `${SPIN[tick % SPIN.length]} working…`, style(0xffcc66));
    s.write(0, 3, `tick ${tick}`, style(0x99ee99));
    for (let y = 5; y < rows - 1; y++) {
      const n = tick + y - 5;
      s.write(0, y, `log line ${String(n).padStart(5)}  ${"·".repeat(n % 40)}`, n % 7 === 0 ? style(0xff8888, null, Flag.BOLD) : dim);
    }
    s.write(0, rows - 1, " static footer ", style(0x181818, 0x88c0d0));
    s.cursor = { ...s.cursor, x: 11, y: 2, in_viewport: true, visible: true };
    return s;
  },
};

const stress: Fixture = {
  name: "stress",
  description: "animated: every cell random text and style each frame",
  interval: 16,
  build(cols, rows, tick, p) {
    const s = new Screen(cols, rows);
    const r = rng(tick + 1);
    // ?colors=16 keeps the palette realistic; the default 256 stresses the glyph cache.
    const ncolors = Number(p.colors ?? 256);
    const chars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789{}[]()<>=+-*/│─┼█▄";
    for (let y = 0; y < rows; y++) {
      let x = 0;
      while (x < cols) {
        const len = 1 + Math.floor(r() * 12);
        const fg = xterm256(Math.floor(r() * ncolors));
        const bg = r() < 0.3 ? xterm256(Math.floor(r() * ncolors)) : null;
        const flags = r() < 0.15 ? Flag.BOLD : r() < 0.05 ? Flag.ITALIC | withUnderline(0, Underline.SINGLE) : 0;
        let t = "";
        for (let i = 0; i < len; i++) t += chars[Math.floor(r() * chars.length)];
        x = s.write(x, y, t.slice(0, cols - x), style(fg, bg, flags));
      }
    }
    s.cursor = { ...s.cursor, x: tick % cols, y: Math.floor(tick / cols) % rows };
    return s;
  },
};

const tui: Fixture = {
  name: "tui",
  description: "an htop-like layout: meters, a table with a selected row, a function-key bar",
  build(cols, rows) {
    const s = new Screen(cols, rows);
    const meter = (y: number, label: string, frac: number, color: number) => {
      s.write(0, y, label.padStart(4) + "[", style(0x88c0d0, null, Flag.BOLD));
      const w = Math.max(10, Math.floor(cols / 2) - 12);
      const n = Math.round(w * frac);
      s.write(5, y, "|".repeat(n), style(color));
      s.write(5 + w, y, `${Math.round(frac * 100)}%]`.padStart(5), style(0xd8d8d8, null, Flag.BOLD));
    };
    meter(0, "1", 0.42, 0xa3be8c);
    meter(1, "2", 0.87, 0xbf616a);
    meter(2, "Mem", 0.63, 0xebcb8b);
    meter(3, "Swp", 0.05, 0xb48ead);
    s.write(Math.floor(cols / 2) + 2, 0, "Tasks: 312, 1204 thr; 3 running", style(0xd8d8d8, null, Flag.BOLD));
    s.write(Math.floor(cols / 2) + 2, 1, "Load average: 2.31 1.97 1.84", style(0xd8d8d8));
    const header = "    PID USER       PRI  NI  VIRT   RES S  CPU% MEM%   TIME+  Command";
    s.fill(0, 5, cols, 1, style(0x181818, 0xa3be8c));
    s.write(0, 5, header.slice(0, cols), style(0x181818, 0xa3be8c));
    const r = rng(7);
    for (let i = 0; i < rows - 7; i++) {
      const y = 6 + i;
      const selected = i === 3;
      const st = selected ? style(0x181818, 0x88c0d0) : style();
      if (selected) s.fill(0, y, cols, 1, st);
      const line = `${String(1000 + i * 37).padStart(7)} chitao      20   0 ${String(Math.floor(r() * 900) + 100).padStart(4)}M ${String(Math.floor(r() * 300)).padStart(4)}M S ${(r() * 40).toFixed(1).padStart(5)} ${(r() * 5).toFixed(1).padStart(4)}  0:${String(Math.floor(r() * 60)).padStart(2, "0")}.12  ${["claude", "node", "cargo", "vim", "zsh"][i % 5]}`;
      s.write(0, y, line.slice(0, cols), st);
    }
    let x = 0;
    for (const [k, label] of [["F1", "Help"], ["F2", "Setup"], ["F3", "Search"], ["F5", "Tree"], ["F9", "Kill"], ["F10", "Quit"]]) {
      x = s.write(x, rows - 1, k, style(0xd8d8d8, 0x181818, Flag.BOLD));
      x = s.write(x, rows - 1, label.padEnd(6), style(0x181818, 0x88c0d0));
    }
    s.fill(x, rows - 1, cols - x, 1, style(0x181818, 0x88c0d0));
    s.cursor = { ...s.cursor, in_viewport: false };
    return s;
  },
};

export const FIXTURES: Fixture[] = [palette, styles, boxdraw, boxjoin, unicode, cursor, tui, partial, stress];

export function fixture(name: string): Fixture | undefined {
  return FIXTURES.find((f) => f.name === name);
}
