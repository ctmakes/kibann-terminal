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
