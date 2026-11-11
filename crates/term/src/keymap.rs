//! W3C `KeyboardEvent.code` to libghostty keys, plus the US-layout
//! unshifted codepoint the Kitty protocol needs.

use crate::vt::key::Key;

/// Map a W3C `code` string to a libghostty key.
pub fn key_from_code(code: &str) -> Key {
    use Key::*;
    match code {
        "Backquote" => Backquote,
        "Backslash" => Backslash,
        "BracketLeft" => BracketLeft,
        "BracketRight" => BracketRight,
        "Comma" => Comma,
        "Digit0" => Digit0,
        "Digit1" => Digit1,
        "Digit2" => Digit2,
        "Digit3" => Digit3,
        "Digit4" => Digit4,
        "Digit5" => Digit5,
        "Digit6" => Digit6,
        "Digit7" => Digit7,
        "Digit8" => Digit8,
        "Digit9" => Digit9,
        "Equal" => Equal,
        "IntlBackslash" => IntlBackslash,
        "IntlRo" => IntlRo,
        "IntlYen" => IntlYen,
        "KeyA" => A,
        "KeyB" => B,
        "KeyC" => C,
        "KeyD" => D,
        "KeyE" => E,
        "KeyF" => F,
        "KeyG" => G,
        "KeyH" => H,
        "KeyI" => I,
        "KeyJ" => J,
        "KeyK" => K,
        "KeyL" => L,
        "KeyM" => M,
        "KeyN" => N,
        "KeyO" => O,
        "KeyP" => P,
        "KeyQ" => Q,
        "KeyR" => R,
        "KeyS" => S,
        "KeyT" => T,
        "KeyU" => U,
        "KeyV" => V,
        "KeyW" => W,
        "KeyX" => X,
        "KeyY" => Y,
        "KeyZ" => Z,
        "Minus" => Minus,
        "Period" => Period,
        "Quote" => Quote,
        "Semicolon" => Semicolon,
        "Slash" => Slash,
        "AltLeft" => AltLeft,
        "AltRight" => AltRight,
        "Backspace" => Backspace,
        "CapsLock" => CapsLock,
        "ContextMenu" => ContextMenu,
        "ControlLeft" => ControlLeft,
        "ControlRight" => ControlRight,
        "Enter" => Enter,
        "MetaLeft" | "OSLeft" => MetaLeft,
        "MetaRight" | "OSRight" => MetaRight,
        "ShiftLeft" => ShiftLeft,
        "ShiftRight" => ShiftRight,
        "Space" => Space,
        "Tab" => Tab,
        "Convert" => Convert,
        "KanaMode" => KanaMode,
        "NonConvert" => NonConvert,
        "Delete" => Delete,
        "End" => End,
        "Help" => Help,
        "Home" => Home,
        "Insert" => Insert,
        "PageDown" => PageDown,
        "PageUp" => PageUp,
        "ArrowDown" => ArrowDown,
        "ArrowLeft" => ArrowLeft,
        "ArrowRight" => ArrowRight,
        "ArrowUp" => ArrowUp,
        "NumLock" => NumLock,
        "Numpad0" => Numpad0,
        "Numpad1" => Numpad1,
        "Numpad2" => Numpad2,
        "Numpad3" => Numpad3,
        "Numpad4" => Numpad4,
        "Numpad5" => Numpad5,
        "Numpad6" => Numpad6,
        "Numpad7" => Numpad7,
        "Numpad8" => Numpad8,
        "Numpad9" => Numpad9,
        "NumpadAdd" => NumpadAdd,
        "NumpadBackspace" => NumpadBackspace,
        "NumpadClear" => NumpadClear,
        "NumpadClearEntry" => NumpadClearEntry,
        "NumpadComma" => NumpadComma,
        "NumpadDecimal" => NumpadDecimal,
        "NumpadDivide" => NumpadDivide,
        "NumpadEnter" => NumpadEnter,
        "NumpadEqual" => NumpadEqual,
        "NumpadMemoryAdd" => NumpadMemoryAdd,
        "NumpadMemoryClear" => NumpadMemoryClear,
        "NumpadMemoryRecall" => NumpadMemoryRecall,
        "NumpadMemoryStore" => NumpadMemoryStore,
        "NumpadMemorySubtract" => NumpadMemorySubtract,
        "NumpadMultiply" => NumpadMultiply,
        "NumpadParenLeft" => NumpadParenLeft,
        "NumpadParenRight" => NumpadParenRight,
        "NumpadSubtract" => NumpadSubtract,
        "Escape" => Escape,
        "F1" => F1,
        "F2" => F2,
        "F3" => F3,
        "F4" => F4,
        "F5" => F5,
        "F6" => F6,
        "F7" => F7,
        "F8" => F8,
        "F9" => F9,
        "F10" => F10,
        "F11" => F11,
        "F12" => F12,
        "F13" => F13,
        "F14" => F14,
        "F15" => F15,
        "F16" => F16,
        "F17" => F17,
        "F18" => F18,
        "F19" => F19,
        "F20" => F20,
        "F21" => F21,
        "F22" => F22,
        "F23" => F23,
        "F24" => F24,
        "Fn" => Fn,
        "FnLock" => FnLock,
        "PrintScreen" => PrintScreen,
        "ScrollLock" => ScrollLock,
        "Pause" => Pause,
        "BrowserBack" => BrowserBack,
        "BrowserFavorites" => BrowserFavorites,
        "BrowserForward" => BrowserForward,
        "BrowserHome" => BrowserHome,
        "BrowserRefresh" => BrowserRefresh,
        "BrowserSearch" => BrowserSearch,
        "BrowserStop" => BrowserStop,
        "Eject" => Eject,
        "LaunchApp1" => LaunchApp1,
        "LaunchApp2" => LaunchApp2,
        "LaunchMail" => LaunchMail,
        "MediaPlayPause" => MediaPlayPause,
        "MediaSelect" => MediaSelect,
        "MediaStop" => MediaStop,
        "MediaTrackNext" => MediaTrackNext,
        "MediaTrackPrevious" => MediaTrackPrevious,
        "Power" => Power,
        "Sleep" => Sleep,
        "AudioVolumeDown" => AudioVolumeDown,
        "AudioVolumeMute" => AudioVolumeMute,
        "AudioVolumeUp" => AudioVolumeUp,
        "WakeUp" => WakeUp,
        "Copy" => Copy,
        "Cut" => Cut,
        "Paste" => Paste,
        _ => Unidentified,
    }
}

/// The character a key produces on a US layout with no modifiers, or `'\0'`.
pub fn unshifted_codepoint(code: &str) -> char {
    if let Some(l) = code.strip_prefix("Key")
        && let [b] = l.as_bytes()
    {
        return (b.to_ascii_lowercase()) as char;
    }
    if let Some(d) = code.strip_prefix("Digit")
        && let [b] = d.as_bytes()
    {
        return *b as char;
    }
    match code {
        "Backquote" => '`',
        "Backslash" | "IntlBackslash" => '\\',
        "BracketLeft" => '[',
        "BracketRight" => ']',
        "Comma" => ',',
        "Equal" => '=',
        "Minus" => '-',
        "Period" => '.',
        "Quote" => '\'',
        "Semicolon" => ';',
        "Slash" => '/',
        "Space" => ' ',
        _ => '\0',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_codes() {
        assert_eq!(key_from_code("KeyA"), Key::A);
        assert_eq!(key_from_code("ArrowUp"), Key::ArrowUp);
        assert_eq!(key_from_code("NumpadEnter"), Key::NumpadEnter);
        assert_eq!(key_from_code("Nope"), Key::Unidentified);
    }

    #[test]
    fn unshifted() {
        assert_eq!(unshifted_codepoint("KeyQ"), 'q');
        assert_eq!(unshifted_codepoint("Digit7"), '7');
        assert_eq!(unshifted_codepoint("Slash"), '/');
        assert_eq!(unshifted_codepoint("Enter"), '\0');
        assert_eq!(unshifted_codepoint("KeyboardX"), '\0');
    }
}
