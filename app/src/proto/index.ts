// Re-exports of the generated protocol types plus the bit constants that ts-rs
// can't express. Keep the constants in sync with `crates/proto/src/lib.rs`.

export type { Frame } from "./generated/Frame";
export type { Style } from "./generated/Style";
export type { Row } from "./generated/Row";
export type { Run } from "./generated/Run";
export type { Cursor } from "./generated/Cursor";
export type { CursorStyle } from "./generated/CursorStyle";
export type { MouseTracking } from "./generated/MouseTracking";
export type { Resize } from "./generated/Resize";
export type { InputEvent } from "./generated/InputEvent";
export type { KeyAction } from "./generated/KeyAction";
export type { MouseAction } from "./generated/MouseAction";
export type { Scrollbar } from "./generated/Scrollbar";
export type { ScrollBy } from "./generated/ScrollBy";
export type { Link } from "./generated/Link";
export type { LinkSpan } from "./generated/LinkSpan";
export type { Layout } from "./generated/Layout";
export type { LayoutTab } from "./generated/LayoutTab";
export type { LayoutAction } from "./generated/LayoutAction";
export type { AgentState } from "./generated/AgentState";
export type { PaneStatus } from "./generated/PaneStatus";

export const Flag = {
  BOLD: 1 << 0,
  ITALIC: 1 << 1,
  FAINT: 1 << 2,
  INVISIBLE: 1 << 3,
  STRIKE: 1 << 4,
  OVERLINE: 1 << 5,
  UNDERLINE_SHIFT: 8,
  UNDERLINE_MASK: 0x7 << 8,
} as const;

export const Underline = {
  NONE: 0,
  SINGLE: 1,
  DOUBLE: 2,
  CURLY: 3,
  DOTTED: 4,
  DASHED: 5,
} as const;

export function underlineOf(flags: number): number {
  return (flags & Flag.UNDERLINE_MASK) >> Flag.UNDERLINE_SHIFT;
}

export function withUnderline(flags: number, ul: number): number {
  return (flags & ~Flag.UNDERLINE_MASK) | (ul << Flag.UNDERLINE_SHIFT);
}

export const Mods = {
  SHIFT: 1 << 0,
  CTRL: 1 << 1,
  ALT: 1 << 2,
  SUPER: 1 << 3,
  CAPS_LOCK: 1 << 4,
  NUM_LOCK: 1 << 5,
  SHIFT_RIGHT: 1 << 6,
  CTRL_RIGHT: 1 << 7,
  ALT_RIGHT: 1 << 8,
  SUPER_RIGHT: 1 << 9,
} as const;
