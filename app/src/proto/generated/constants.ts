// Generated from crates/proto/src/lib.rs by `cargo test -p proto`. Do not edit.

export const Flag = {
  BOLD: 1,
  ITALIC: 2,
  FAINT: 4,
  INVISIBLE: 8,
  STRIKE: 16,
  OVERLINE: 32,
  UNDERLINE_SHIFT: 8,
  UNDERLINE_MASK: 1792,
} as const;

export const Mods = {
  SHIFT: 1,
  CTRL: 2,
  ALT: 4,
  SUPER: 8,
  CAPS_LOCK: 16,
  NUM_LOCK: 32,
  SHIFT_RIGHT: 64,
  CTRL_RIGHT: 128,
  ALT_RIGHT: 256,
  SUPER_RIGHT: 512,
} as const;

export const Underline = {
  NONE: 0,
  SINGLE: 1,
  DOUBLE: 2,
  CURLY: 3,
  DOTTED: 4,
  DASHED: 5,
} as const;

/** See `CHROME_MIX` in crates/proto/src/theme.rs. */
export const MIX = {
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

/** Luminance below which a theme is dark. */
export const DARK_BELOW = 0.4;

/** The parts API's version (`kibann.api`). */
export const PART_API = 1;
