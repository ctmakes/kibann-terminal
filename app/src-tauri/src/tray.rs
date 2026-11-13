//! The menu-bar icon (macOS). With it, closing the window only hides it:
//! the app keeps running, so agents in the shells can still notify, and the
//! icon brings the window back. Its Quit ends everything: the shells, the
//! daemon and the app. `menu-bar-icon = false` in the config turns it off
//! (closing the window quits the app, as before).
//!
//! The icon shows a dot while an agent waits for the user; its menu says
//! how many, the version, and a downloaded update.

use std::sync::Mutex;

use tauri::image::Image;
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Wry};

const ID: &str = "main";

/// The menu's item ids: all start with "tray-", which the app's menu
/// handler (it gets every menu's events) passes to [`on_menu`].
pub const PREFIX: &str = "tray-";
const VERSION: &str = "tray-version";
const SHOW: &str = "tray-show";
const NEW_TAB: &str = "tray-new-tab";
const WAITING: &str = "tray-waiting";
const UPDATE: &str = "tray-update";
const CHECK: &str = "tray-check";
const QUIT: &str = "tray-quit";

/// What the icon and its menu show.
#[derive(Default)]
struct Shown {
    /// Agents waiting for the user (the Dock badge's count).
    waiting: u32,
    /// A downloaded version, waiting for a restart.
    update: Option<String>,
}

#[derive(Default)]
pub struct Tray(Mutex<Shown>);

/// One line of the menu.
#[derive(Debug, PartialEq, Eq)]
enum Entry {
    Item {
        id: &'static str,
        text: String,
        enabled: bool,
    },
    Separator,
}

fn item(id: &'static str, text: impl Into<String>) -> Entry {
    Entry::Item {
        id,
        text: text.into(),
        enabled: true,
    }
}

/// The menu for this state.
fn entries(version: &str, waiting: u32, update: Option<&str>) -> Vec<Entry> {
    let mut e = vec![
        Entry::Item {
            id: VERSION,
            text: format!("Kibann Terminal {version}"),
            enabled: false,
        },
        Entry::Separator,
        item(SHOW, "Show Kibann Terminal"),
        item(NEW_TAB, "New Tab"),
    ];
    if waiting > 0 {
        e.push(Entry::Separator);
        let text = if waiting == 1 {
            "1 agent is waiting".to_owned()
        } else {
            format!("{waiting} agents are waiting")
        };
        e.push(item(WAITING, text));
    }
    e.push(Entry::Separator);
    e.push(match update {
        Some(v) => item(UPDATE, format!("Restart to Update to {v}")),
        None => item(CHECK, "Check for Updates…"),
    });
    e.push(Entry::Separator);
    e.push(item(QUIT, "Quit Kibann Terminal"));
    e
}

fn menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let shown = app.state::<Tray>();
    let (waiting, update) = {
        let s = shown.0.lock().unwrap();
        (s.waiting, s.update.clone())
    };
    let version = app.package_info().version.to_string();
    let mut m = MenuBuilder::new(app);
    for e in entries(&version, waiting, update.as_deref()) {
        m = match e {
            Entry::Separator => m.separator(),
            Entry::Item { id, text, enabled } => m.item(
                &MenuItemBuilder::with_id(id, text)
                    .enabled(enabled)
                    .build(app)?,
            ),
        };
    }
    m.build()
}

fn icon(waiting: bool) -> tauri::Result<Image<'static>> {
    Image::from_bytes(if waiting {
        include_bytes!("../icons/tray-dot.png")
    } else {
        include_bytes!("../icons/tray.png")
    })
}

/// Make the icon (hidden when the config turns it off).
pub fn create(app: &AppHandle, visible: bool) -> tauri::Result<()> {
    app.manage(Tray::default());
    let tray = TrayIconBuilder::with_id(ID)
        .icon(icon(false)?)
        .icon_as_template(true)
        .tooltip("Kibann Terminal")
        .menu(&menu(app)?)
        .show_menu_on_left_click(true)
        .build(app)?;
    if cfg!(debug_assertions) {
        eprintln!("kibann: menu-bar icon made (shown: {visible})");
    }
    tray.set_visible(visible)
}

/// Show or hide the icon (the config changed).
pub fn set_visible(app: &AppHandle, visible: bool) {
    if let Some(t) = app.tray_by_id(ID) {
        let _ = t.set_visible(visible);
    }
}

/// Whether closing the window should only hide it.
pub fn enabled(app: &AppHandle) -> bool {
    app.state::<Mutex<proto::config::Settings>>()
        .lock()
        .unwrap()
        .menu_bar_icon
}

fn refresh(app: &AppHandle, waiting: bool) {
    let Some(t) = app.tray_by_id(ID) else {
        return;
    };
    if let Ok(m) = menu(app) {
        let _ = t.set_menu(Some(m));
    }
    if let Ok(i) = icon(waiting) {
        let _ = t.set_icon(Some(i));
        // Setting the image drops its template flag.
        let _ = t.set_icon_as_template(true);
    }
}

/// How many agents wait for the user.
pub fn set_waiting(app: &AppHandle, waiting: u32) {
    let Some(tray) = app.try_state::<Tray>() else {
        return;
    };
    {
        let mut s = tray.0.lock().unwrap();
        if s.waiting == waiting {
            return;
        }
        s.waiting = waiting;
    }
    refresh(app, waiting > 0);
}

/// A version was downloaded and installs on restart.
pub fn set_update(app: &AppHandle, version: String) {
    let Some(tray) = app.try_state::<Tray>() else {
        return;
    };
    let waiting = {
        let mut s = tray.0.lock().unwrap();
        s.update = Some(version);
        s.waiting
    };
    refresh(app, waiting > 0);
}

/// Bring the window back (from the icon, the Dock or a notification).
pub fn show_window(app: &AppHandle) {
    if cfg!(debug_assertions) {
        eprintln!("kibann: window shown");
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    // An empty window (its last pane was closed) opens a shell.
    let _ = app.emit("window-shown", ());
}

/// Close the window but keep running.
pub fn hide_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let r = w.hide();
        if cfg!(debug_assertions) {
            eprintln!(
                "kibann: window hidden: {r:?}, visible now: {:?}",
                w.is_visible()
            );
        }
    }
}

/// A click in the icon's menu.
pub fn on_menu(app: &AppHandle, id: &str) {
    match id {
        SHOW => show_window(app),
        NEW_TAB => {
            show_window(app);
            let _ = app.emit("menu-action", r#"{"kind":"newTab"}"#);
        }
        WAITING => {
            show_window(app);
            let _ = app.emit("show-waiting", ());
        }
        UPDATE => {
            if let Err(e) = crate::update::update_install(app.clone()) {
                show_window(app);
                let _ = app.emit("notice", format!("Couldn't update: {e}"));
            }
        }
        CHECK => {
            // The answer is a notice in the window.
            show_window(app);
            let app = app.clone();
            tauri::async_runtime::spawn(async move { crate::update::check(&app, true).await });
        }
        QUIT => quit(app),
        _ => {}
    }
}

/// Ask first when shells would end, then end everything.
fn quit(app: &AppHandle) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let open = app
        .try_state::<crate::Term>()
        .and_then(|t| t.client.layout().ok())
        .map_or(0, |l| l.panes().len());
    if open == 0 {
        crate::quit_ending_sessions(app);
        return;
    }
    show_window(app);
    let handle = app.clone();
    app.dialog()
        .message(quit_text(open))
        .title("Quit Kibann Terminal?")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Quit".into(),
            "Cancel".into(),
        ))
        .show(move |quit| {
            if quit {
                crate::quit_ending_sessions(&handle);
            }
        });
}

fn quit_text(open: usize) -> String {
    let what = if open == 1 {
        "1 terminal is open. It ends, with whatever runs in it".to_owned()
    } else {
        format!("{open} terminals are open. They end, with whatever runs in them")
    };
    format!(
        "{what}, Claude Code sessions too.\n\n\
         The tabs and splits come back next time, with fresh shells. To keep \
         the shells running, close the window instead."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(e: &[Entry]) -> Vec<&str> {
        e.iter()
            .map(|e| match e {
                Entry::Item { id, .. } => *id,
                Entry::Separator => "-",
            })
            .collect()
    }

    #[test]
    fn the_menu_follows_the_state() {
        let quiet = entries("0.1.0", 0, None);
        assert_eq!(
            ids(&quiet),
            [VERSION, "-", SHOW, NEW_TAB, "-", CHECK, "-", QUIT]
        );
        assert_eq!(
            quiet[0],
            Entry::Item {
                id: VERSION,
                text: "Kibann Terminal 0.1.0".into(),
                enabled: false
            }
        );
        let busy = entries("0.1.0", 2, Some("0.1.1"));
        assert_eq!(
            ids(&busy),
            [
                VERSION, "-", SHOW, NEW_TAB, "-", WAITING, "-", UPDATE, "-", QUIT
            ]
        );
        assert!(busy.contains(&item(WAITING, "2 agents are waiting")));
        assert!(busy.contains(&item(UPDATE, "Restart to Update to 0.1.1")));
        assert!(entries("0.1.0", 1, None).contains(&item(WAITING, "1 agent is waiting")));
        assert!(entries("0", 0, None).iter().all(|e| match e {
            Entry::Item { id, .. } => id.starts_with(PREFIX),
            Entry::Separator => true,
        }));
    }

    #[test]
    fn quitting_says_what_ends() {
        assert!(quit_text(1).starts_with("1 terminal is open. It ends,"));
        assert!(quit_text(3).starts_with("3 terminals are open. They end,"));
    }
}
