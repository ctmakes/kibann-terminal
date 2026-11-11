//! Messages exchanged between the terminal backend and the web renderer.
//!
//! The Rust types here are the single source of truth. `cargo test -p proto`
//! regenerates the TypeScript copies in `app/src/proto/generated/`.
//!
//! Units: sizes and positions are device pixels unless counted in cells.
//! Colors are `0xRRGGBB`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Bits of [`Style::flags`].
pub mod flags {
    pub const BOLD: u16 = 1 << 0;
    pub const ITALIC: u16 = 1 << 1;
    pub const FAINT: u16 = 1 << 2;
    pub const INVISIBLE: u16 = 1 << 3;
    pub const STRIKE: u16 = 1 << 4;
    pub const OVERLINE: u16 = 1 << 5;
    /// Underline style lives in bits 8..=10, see [`Underline`](super::Underline).
    pub const UNDERLINE_SHIFT: u16 = 8;
    pub const UNDERLINE_MASK: u16 = 0x7 << UNDERLINE_SHIFT;
}

/// Bits of `mods` on input events. Same layout as libghostty's `GhosttyMods`.
pub mod mods {
    pub const SHIFT: u16 = 1 << 0;
    pub const CTRL: u16 = 1 << 1;
    pub const ALT: u16 = 1 << 2;
    pub const SUPER: u16 = 1 << 3;
    pub const CAPS_LOCK: u16 = 1 << 4;
    pub const NUM_LOCK: u16 = 1 << 5;
    /// Set when the right-hand key of that modifier is the one held.
    pub const SHIFT_RIGHT: u16 = 1 << 6;
    pub const CTRL_RIGHT: u16 = 1 << 7;
    pub const ALT_RIGHT: u16 = 1 << 8;
    pub const SUPER_RIGHT: u16 = 1 << 9;
}

/// Underline styles stored in [`Style::flags`]. Values match `GhosttySgrUnderline`.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Underline {
    None = 0,
    Single = 1,
    Double = 2,
    Curly = 3,
    Dotted = 4,
    Dashed = 5,
}

/// One update of the screen, sent backend → frontend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Frame {
    /// Increments by one per frame. A gap means the frontend must ask for a full frame.
    #[ts(type = "number")]
    pub seq: u64,
    /// `rows` covers every row; the frontend replaces its mirror.
    pub full: bool,
    pub cols: u16,
    pub rows_total: u16,
    pub default_fg: u32,
    pub default_bg: u32,
    /// Style table local to this frame. Runs refer to it by index.
    pub styles: Vec<Style>,
    /// Changed rows only, unless `full`.
    pub rows: Vec<Row>,
    pub cursor: Cursor,
    pub mouse_tracking: MouseTracking,
    pub focus_reporting: bool,
}

/// Fully resolved cell style. Inverse is already applied and blink is dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Style {
    /// Resolved foreground, never default.
    pub fg: u32,
    /// `None` means the default background.
    pub bg: Option<u32>,
    /// Underline color. `None` means the foreground.
    pub ul: Option<u32>,
    /// See [`flags`].
    pub flags: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Row {
    pub y: u16,
    /// Runs together cover columns `0..cols` with no gaps, in order.
    pub runs: Vec<Run>,
}

/// Consecutive cells with the same style.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Run {
    pub x: u16,
    pub len: u16,
    /// Index into [`Frame::styles`].
    pub s: u16,
    /// Exactly `len` entries, one grapheme cluster per column; `""` is an empty
    /// cell or the tail of a wide character. Omitted when every cell is blank.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub text: Option<Vec<String>>,
    /// Offsets within the run whose cell is a wide character. The next entry is
    /// its tail. Omitted when there are none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub wide: Option<Vec<u16>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub in_viewport: bool,
    pub visible: bool,
    pub blinking: bool,
    pub style: CursorStyle,
    /// The cursor sits on a wide character, so it is drawn two cells wide.
    pub wide: bool,
    /// Explicit cursor color set by the program.
    pub color: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CursorStyle {
    Bar,
    Block,
    Underline,
    BlockHollow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MouseTracking {
    None,
    Normal,
    Button,
    Any,
}

/// Grid size, sent frontend → backend (`term_resize`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Resize {
    pub cols: u16,
    pub rows: u16,
    /// Cell size in device pixels, for `ghostty_terminal_resize` and the mouse encoder.
    pub cell_w: u16,
    pub cell_h: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum KeyAction {
    Press,
    Repeat,
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MouseAction {
    Press,
    Release,
    Motion,
}

/// Input sent frontend → backend (`term_input`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum InputEvent {
    Key {
        action: KeyAction,
        /// W3C `KeyboardEvent.code`, which maps onto `GhosttyKey`.
        code: String,
        /// W3C `KeyboardEvent.key`.
        key: String,
        /// Text the key produces, if any.
        text: Option<String>,
        /// See [`mods`].
        mods: u16,
        composing: bool,
    },
    /// Finished IME text, emoji picker or dictation output.
    Text { data: String },
    /// Clipboard paste; the backend applies bracketed paste.
    Paste { data: String },
    Mouse {
        action: MouseAction,
        /// libghostty mouse button number (1 left, 2 right, 3 middle, 8 back, 9 forward).
        button: Option<u8>,
        /// Device pixels from the grid's top-left corner.
        x: f32,
        y: f32,
        mods: u16,
    },
    Wheel {
        /// Whole rows scrolled; positive is right / down.
        dx_rows: i16,
        dy_rows: i16,
        x: f32,
        y: f32,
        mods: u16,
    },
    Focus { focused: bool },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_omits_blank_fields() {
        let run = Run { x: 0, len: 4, s: 0, text: None, wide: None };
        assert_eq!(serde_json::to_string(&run).unwrap(), r#"{"x":0,"len":4,"s":0}"#);
    }

    #[test]
    fn input_event_is_tagged_by_kind() {
        let ev = InputEvent::Focus { focused: true };
        assert_eq!(serde_json::to_string(&ev).unwrap(), r#"{"kind":"focus","focused":true}"#);
        let back: InputEvent = serde_json::from_str(r#"{"kind":"text","data":"é"}"#).unwrap();
        assert_eq!(back, InputEvent::Text { data: "é".into() });
    }

    #[test]
    fn underline_bits_fit_mask() {
        let f = (Underline::Dashed as u16) << flags::UNDERLINE_SHIFT;
        assert_eq!(f & !flags::UNDERLINE_MASK, 0);
    }
}
