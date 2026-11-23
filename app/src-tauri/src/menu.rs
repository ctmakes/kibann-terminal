//! The macOS menu bar, the quit items, and the page's shortcuts shown on
//! the menu's items.

use proto::wire::ClientMsg;
use tauri::{AppHandle, Manager};

use crate::link::Term;

/// The shortcuts changed (the config's `keybind`): show each menu item's
/// new key, or none, so a key the config moved elsewhere doesn't also run
/// the menu item it used to. `keys` maps the items' ids (actions as JSON).
#[tauri::command]
pub fn menu_keys(app: AppHandle, keys: std::collections::HashMap<String, Option<String>>) {
    use tauri::menu::MenuItemKind;
    fn walk(
        items: Vec<MenuItemKind<tauri::Wry>>,
        keys: &std::collections::HashMap<String, Option<String>>,
    ) {
        for item in items {
            match item {
                MenuItemKind::Submenu(sub) => walk(sub.items().unwrap_or_default(), keys),
                MenuItemKind::MenuItem(i) => {
                    if let Some(k) = keys.get(&i.id().0)
                        && let Err(e) = i.set_accelerator(k.as_deref())
                    {
                        eprintln!("kibann: menu key {k:?}: {e}");
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(menu) = app.menu() {
        walk(menu.items().unwrap_or_default(), &keys);
    }
}

/// End every shell and the daemon, then quit.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn quit_ending_sessions(app: &AppHandle) {
    if let Some(t) = app.try_state::<Term>() {
        let _ = t.send(ClientMsg::Shutdown);
    }
    app.exit(0);
}

/// Menu item id for [`quit_ending_sessions`].
#[cfg(target_os = "macos")]
pub(crate) const QUIT_END_SESSIONS: &str = "quit-end-sessions";
/// Menu item id for Cmd+Q: with the menu-bar icon it only closes the window
/// (the app keeps watching agents), without it it quits the app.
#[cfg(target_os = "macos")]
pub(crate) const QUIT_KEY: &str = "quit-key";

/// The app menu's two quits say what they do, which depends on whether the
/// app keeps running in the menu bar.
#[cfg(target_os = "macos")]
pub(crate) fn quit_labels(menu_bar_icon: bool) -> [(&'static str, &'static str); 2] {
    if menu_bar_icon {
        [
            (QUIT_KEY, "Close to Menu Bar"),
            (QUIT_END_SESSIONS, "Quit Kibann Terminal…"),
        ]
    } else {
        [
            (QUIT_KEY, "Quit Kibann Terminal"),
            (QUIT_END_SESSIONS, "Quit and End Sessions…"),
        ]
    }
}

/// Retitle the app menu's quits for the setting.
#[cfg(target_os = "macos")]
pub(crate) fn set_quit_labels(app: &AppHandle, menu_bar_icon: bool) {
    use tauri::menu::MenuItemKind;
    let Some(menu) = app.menu() else { return };
    for (id, text) in quit_labels(menu_bar_icon) {
        let found = menu
            .items()
            .unwrap_or_default()
            .into_iter()
            .find_map(|m| match m {
                MenuItemKind::Submenu(sub) => sub.get(id),
                _ => None,
            });
        if let Some(MenuItemKind::MenuItem(i)) = found {
            let _ = i.set_text(text);
        }
    }
}
/// Menu item id for Check for Updates….
#[cfg(target_os = "macos")]
pub(crate) const CHECK_UPDATES: &str = "check-updates";
/// Menu item id for Help → What's New.
#[cfg(target_os = "macos")]
pub(crate) const WHATS_NEW: &str = "whats-new";

/// The macOS menu bar: the standard menus plus the workspace actions with
/// their shortcuts. An item's id is its `Command` as JSON.
#[cfg(target_os = "macos")]
pub(crate) fn menu_bar(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use proto::command::Command;
    use proto::layout::Dir;
    use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
    let item = |id: &str, text: &str, accel: &str| {
        MenuItemBuilder::with_id(id, text)
            .accelerator(accel)
            .build(app)
    };
    let cmd = |c: Command, text: &str, accel: &str| item(&c.id(), text, accel);
    let app_menu = SubmenuBuilder::new(app, "Kibann Terminal")
        .about(None)
        .separator()
        .item(&cmd(Command::Settings, "Settings…", "Cmd+Comma")?)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .item(&MenuItemBuilder::with_id(CHECK_UPDATES, "Check for Updates…").build(app)?)
        .separator()
        // The settings aren't read yet: these say what they do with the
        // default (the menu-bar icon on) until they are.
        .item(&MenuItemBuilder::with_id(QUIT_END_SESSIONS, quit_labels(true)[1].1).build(app)?)
        .item(&item(QUIT_KEY, quit_labels(true)[0].1, "Cmd+Q")?)
        .build()?;
    let shell = SubmenuBuilder::new(app, "Shell")
        .item(&cmd(Command::NewTab, "New Tab", "Cmd+T")?)
        .separator()
        .item(&cmd(
            Command::Split { dir: Dir::Row },
            "Split Right",
            "Cmd+D",
        )?)
        .item(&cmd(
            Command::Split { dir: Dir::Column },
            "Split Down",
            "Shift+Cmd+D",
        )?)
        .separator()
        .item(&cmd(Command::Close, "Close", "Cmd+W")?)
        .build()?;
    let edit = SubmenuBuilder::new(app, "Edit")
        .copy()
        .paste()
        .separator()
        .item(&cmd(Command::Find, "Find…", "Cmd+F")?)
        .item(&cmd(
            Command::FindStep { older: true },
            "Find Next",
            "Cmd+G",
        )?)
        .item(&cmd(
            Command::FindStep { older: false },
            "Find Previous",
            "Shift+Cmd+G",
        )?)
        .build()?;
    let view = SubmenuBuilder::new(app, "View")
        .item(&cmd(Command::Sidebar, "Toggle Sidebar", "Shift+Cmd+B")?)
        .item(&MenuItemBuilder::with_id(Command::Themes.id(), "Theme…").build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id(Command::Workspaces.id(), "Open Workspace…").build(app)?)
        .item(&MenuItemBuilder::with_id(Command::SaveWorkspace.id(), "Save Workspace…").build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id(Command::Screens.id(), "Screen…").build(app)?)
        .item(&cmd(
            Command::ScreenCycle { step: 1 },
            "Next Screen",
            "Shift+Cmd+L",
        )?)
        .item(&cmd(
            Command::ScreenCycle { step: -1 },
            "Previous Screen",
            "Shift+Cmd+K",
        )?)
        .separator()
        .item(&cmd(Command::FontSize { step: 1 }, "Bigger", "Cmd+Equal")?)
        .item(&cmd(
            Command::FontSize { step: -1 },
            "Smaller",
            "Cmd+Minus",
        )?)
        .item(&cmd(
            Command::FontSize { step: 0 },
            "Actual Size",
            "Cmd+Digit0",
        )?)
        .separator()
        .item(&cmd(Command::Zoom, "Zoom Split", "Shift+Cmd+Enter")?)
        .item(&cmd(Command::Equalize, "Equalize Splits", "Ctrl+Cmd+=")?)
        .separator()
        .fullscreen()
        .build()?;
    let window = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .separator()
        .item(&cmd(
            Command::TabCycle { step: -1 },
            "Show Previous Tab",
            "Shift+Cmd+[",
        )?)
        .item(&cmd(
            Command::TabCycle { step: 1 },
            "Show Next Tab",
            "Shift+Cmd+]",
        )?)
        .separator()
        .item(&cmd(
            Command::FocusCycle { step: -1 },
            "Select Previous Split",
            "Cmd+[",
        )?)
        .item(&cmd(
            Command::FocusCycle { step: 1 },
            "Select Next Split",
            "Cmd+]",
        )?)
        .build()?;
    let help = SubmenuBuilder::new(app, "Help")
        .item(&cmd(Command::Shortcuts, "Keyboard Shortcuts", "Cmd+/")?)
        .item(&MenuItemBuilder::with_id(WHATS_NEW, "What's New").build(app)?)
        .build()?;
    MenuBuilder::new(app)
        .items(&[&app_menu, &shell, &edit, &view, &window, &help])
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn the_quits_say_what_they_do() {
        assert_eq!(quit_labels(true)[0], (QUIT_KEY, "Close to Menu Bar"));
        assert_eq!(quit_labels(true)[1].1, "Quit Kibann Terminal…");
        assert_eq!(quit_labels(false)[0].1, "Quit Kibann Terminal");
        assert_eq!(quit_labels(false)[1].1, "Quit and End Sessions…");
    }
}
