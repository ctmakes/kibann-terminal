// The window's colors from the terminal theme. Every surface and text color
// is the theme's background mixed toward its foreground; on the built-in
// dark theme (black, white) the mixes give back the greys the app always
// had. Status and git colors keep their meaning, in a set for dark and a
// set for light backgrounds.

import type { Theme } from "../proto";

/** How far toward the foreground each surface or text color sits. */
const MIX = {
  sidebar: 0.063,
  bar: 0.067,
  header: 0.078,
  panel: 0.1,
  tab: 0.114,
  tabActive: 0.165,
  kbd: 0.165,
  edge: 0.165,
  divider: 0.227,
  muted: 0.49,
  text: 0.604,
  label: 0.66,
  strong: 0.784,
  title: 0.847,
  bright: 0.9,
} as const;

/** Surfaces text is drawn on, and the text colors that must read on them. */
const SURFACES = ["sidebar", "bar", "header", "panel"] as const;
const TEXT = ["text", "label", "strong", "title", "bright"] as const;

const DARK = {
  working: "#3fb950",
  blocked: "#e3a008",
  done: "#58a6ff",
  changed: "#d29922",
  conflicts: "#f85149",
  op: "#f0883e",
};

const LIGHT = {
  working: "#1a7f37",
  blocked: "#9a6700",
  done: "#0969da",
  changed: "#9a6700",
  conflicts: "#cf222e",
  op: "#bc4c00",
};

const hex = (c: number) => `#${(c & 0xffffff).toString(16).padStart(6, "0")}`;

/** `a` moved toward `b` by `t` (0..1), per channel. */
export function mix(a: number, b: number, t: number): number {
  const ch = (s: number) => Math.round(((a >> s) & 0xff) * (1 - t) + ((b >> s) & 0xff) * t);
  return (ch(16) << 16) | (ch(8) << 8) | ch(0);
}

/** WCAG relative luminance, 0..1. */
export function luminance(c: number): number {
  const ch = (s: number) => {
    const v = ((c >> s) & 0xff) / 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * ch(16) + 0.7152 * ch(8) + 0.0722 * ch(0);
}

/** WCAG contrast ratio, 1..21. */
export function contrast(a: number, b: number): number {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

export const isDark = (t: Theme) => luminance(t.background) < 0.4;

/** The CSS variables (`--k-…`) for `theme`. */
export function chromeFor(t: Theme): Record<string, string> {
  const { background: bg, foreground: fg } = t;
  const v: Record<string, string> = {};
  for (const [name, at] of Object.entries(MIX)) v[`--k-${name}`] = hex(mix(bg, fg, at));
  // A low-contrast theme would leave secondary text too faint: move each
  // text color toward the foreground until it reads (4.5:1) on every
  // surface it sits on.
  const surfaces = SURFACES.map((s) => mix(bg, fg, MIX[s]));
  for (const name of TEXT) {
    let at: number = MIX[name];
    const reads = () => surfaces.every((s) => contrast(mix(bg, fg, at), s) >= 4.5);
    while (at < 1 && !reads()) at = Math.min(1, at + 0.02);
    v[`--k-${name}`] = hex(mix(bg, fg, at));
  }
  const dark = isDark(t);
  for (const [name, c] of Object.entries(dark ? DARK : LIGHT)) v[`--k-${name}`] = c;
  v["--k-bg"] = hex(bg);
  v["--k-fg"] = hex(fg);
  // The focused row in the sidebar: a tint of the theme's blue.
  v["--k-current"] = hex(mix(bg, t.palette[4] ?? 0x58a6ff, dark ? 0.32 : 0.22));
  v["--k-current-text"] = hex(dark ? mix(fg, 0xffffff, 0.5) : mix(fg, 0x000000, 0.3));
  v["--k-scrim"] = dark ? "rgba(0,0,0,0.55)" : "rgba(0,0,0,0.25)";
  v["--k-shadow"] = dark ? "rgba(0,0,0,0.55)" : "rgba(0,0,0,0.18)";
  return v;
}

/** Put `theme`'s variables on `el` (the document by default). */
export function applyChrome(t: Theme, el: HTMLElement = document.documentElement): void {
  for (const [k, val] of Object.entries(chromeFor(t))) el.style.setProperty(k, val);
  el.style.colorScheme = isDark(t) ? "dark" : "light";
}

/** The built-in dark theme, used until the app sends one (and in tests). */
export const KIBANN_DARK: Theme = {
  palette: [
    0x1d1f21, 0xcc6666, 0xb5bd68, 0xf0c674, 0x81a2be, 0xb294bb, 0x8abeb7, 0xc5c8c6, 0x666666, 0xd54e53, 0xb9ca4a,
    0xe7c547, 0x7aa6da, 0xc397d8, 0x70c0b1, 0xeaeaea,
  ],
  background: 0x000000,
  foreground: 0xffffff,
  cursor: null,
  cursor_text: null,
  selection_bg: null,
  selection_fg: null,
};

/** A theme in the list the picker shows. */
export interface ThemeEntry {
  name: string;
  dark: boolean;
}

/** What the workspace needs to list, preview and keep themes. */
export interface ThemeApi {
  list(): Promise<ThemeEntry[]>;
  /** Show `name` without keeping it; `null` goes back to the kept one. */
  preview(name: string | null): Promise<void>;
  /** Keep `name` (for the current appearance) and save it. */
  choose(name: string): Promise<void>;
  /** The theme in use now. */
  current(): string;
}
