//! Double-clicking the title bar (macOS). The window's title bar is a real
//! one, made transparent to show the theme's color (`tauri.conf.json`), but
//! with the web view under it a double-click there doesn't zoom as it does
//! in other apps. This does what System Settings → Desktop & Dock → "Double-
//! click a window's title bar to" says: zoom (the default), minimize, or
//! nothing.

use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{NSEvent, NSEventMask, NSWindow, NSWindowButton};
use objc2_foundation::{NSRect, NSString, NSUserDefaults};

/// What a double-click on the title bar does.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Zoom,
    Minimize,
    Nothing,
}

/// The system setting: `AppleActionOnDoubleClick` ("Maximize", "Fill",
/// "Minimize", "None"), or before it existed `AppleMiniaturizeOnDoubleClick`.
pub fn action(setting: Option<&str>, old_minimize: bool) -> Action {
    match setting {
        Some("Minimize") => Action::Minimize,
        Some("None") => Action::Nothing,
        Some(_) => Action::Zoom,
        None if old_minimize => Action::Minimize,
        None => Action::Zoom,
    }
}

fn system_action() -> Action {
    let d = NSUserDefaults::standardUserDefaults();
    let setting = d
        .stringForKey(&NSString::from_str("AppleActionOnDoubleClick"))
        .map(|s| s.to_string());
    let old = d.boolForKey(&NSString::from_str("AppleMiniaturizeOnDoubleClick"));
    action(setting.as_deref(), old)
}

fn contains(r: NSRect, x: f64, y: f64) -> bool {
    x >= r.origin.x
        && x < r.origin.x + r.size.width
        && y >= r.origin.y
        && y < r.origin.y + r.size.height
}

/// Watch for double-clicks on `window`'s title bar (on the main thread).
pub fn install(window: &tauri::WebviewWindow) {
    let Ok(ptr) = window.ns_window() else {
        return;
    };
    // SAFETY: Tauri's NSWindow, alive as long as the app (the main window
    // is hidden, never closed); retained here for the monitor.
    let Some(ns) = (unsafe { Retained::retain(ptr.cast::<NSWindow>()) }) else {
        return;
    };
    let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit passes a valid event.
        let e = unsafe { event.as_ref() };
        if e.clickCount() != 2 || e.windowNumber() != ns.windowNumber() {
            return event.as_ptr();
        }
        let at = e.locationInWindow();
        // Above the content, and not on the window's buttons.
        let content = ns.contentLayoutRect();
        if at.y < content.origin.y + content.size.height {
            return event.as_ptr();
        }
        for b in [
            NSWindowButton::CloseButton,
            NSWindowButton::MiniaturizeButton,
            NSWindowButton::ZoomButton,
        ] {
            if let Some(button) = ns.standardWindowButton(b)
                && contains(button.convertRect_toView(button.bounds(), None), at.x, at.y)
            {
                return event.as_ptr();
            }
        }
        match system_action() {
            Action::Zoom => ns.zoom(None),
            Action::Minimize => ns.miniaturize(None),
            Action::Nothing => {}
        }
        // Handled: AppKit doesn't also act on it.
        std::ptr::null_mut()
    });
    // SAFETY: the handler returns the event or null, as the API asks. The
    // monitor lives as long as the app.
    let monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::LeftMouseDown, &handler)
    };
    std::mem::forget(monitor);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_system_setting() {
        assert_eq!(action(None, false), Action::Zoom);
        assert_eq!(action(None, true), Action::Minimize);
        assert_eq!(action(Some("Maximize"), true), Action::Zoom);
        assert_eq!(action(Some("Fill"), false), Action::Zoom);
        assert_eq!(action(Some("Minimize"), false), Action::Minimize);
        assert_eq!(action(Some("None"), false), Action::Nothing);
    }
}
