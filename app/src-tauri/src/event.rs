//! The events the app sends the page. `cargo test` writes their names to
//! `app/src/proto/generated/tauri.ts`, with the commands', so the page
//! listens only for events that exist.

macro_rules! events {
    ($($(#[$d:meta])* $c:ident = $v:literal;)*) => {
        $($(#[$d])* pub const $c: &str = $v;)*
        /// Every event's name.
        #[cfg(test)]
        pub const ALL: &[&str] = &[$($v),*];
    };
}

events! {
    /// The layout (`Layout`).
    LAYOUT = "layout";
    /// Every pane's agent status (`StatusMap`).
    AGENT_STATUS = "agent-status";
    /// Every pane's directory and git (`GitMap`).
    PANE_GIT = "pane-git";
    /// A message to show for a while (string).
    NOTICE = "notice";
    /// The theme in use (`ThemeNow`).
    THEME = "theme";
    /// The config file's path and problems (`ConfigInfo`).
    CONFIG_INFO = "config-info";
    /// The config's settings (`Settings`).
    SETTINGS = "settings";
    /// A menu item or tray item to run (a keymap `Action` as JSON).
    MENU_ACTION = "menu-action";
    /// Show what's new.
    WHATS_NEW = "whats-new";
    /// Show a pane (a notification was clicked).
    FOCUS_PANE = "focus-pane";
    /// The screen in use (`ScreenNow`).
    SCREEN = "screen";
    /// The window was shown again from the tray.
    WINDOW_SHOWN = "window-shown";
    /// Show the panes waiting for the user.
    SHOW_WAITING = "show-waiting";
    /// Where updating stands (`UpdateStatus`).
    UPDATE_STATUS = "update-status";
    /// An update is downloaded (`UpdateReady`).
    UPDATE_READY = "update-ready";
    /// The terminal service went away, and its shells with it.
    SERVICE_LOST = "service-lost";
    /// A terminal service runs again (the panes reopened, a number).
    SERVICE_BACK = "service-back";
}
