//! Tauri shell: one window, any number of terminal sessions (one per pane).
//!
//! Sessions and the layout live in the daemon. The link to it is
//! [`link`]; the theme, the config and the menu bar have their own modules;
//! this file holds the list of commands and starts the app.

mod config;
mod event;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod fonts;
mod link;
mod looks;
mod menu;
mod notify;
mod open;
mod screens;
mod service;
#[cfg(target_os = "macos")]
mod titlebar;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod tray;
mod update;

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};

use config::{Problems, reload_config, watch_config};
use link::{Term, events};
use looks::{Looks, show_theme};
#[cfg(target_os = "macos")]
use menu::{CHECK_UPDATES, QUIT_END_SESSIONS, QUIT_KEY, WHATS_NEW, menu_bar};

/// Every command the page can invoke. `cargo test` writes their names to
/// `app/src/proto/generated/tauri.ts`, so the page calls only commands that
/// exist.
macro_rules! commands {
    ($($c:ident $(:: $d:ident)*),* $(,)?) => {
        fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
            tauri::generate_handler![$($c $(:: $d)*),*]
        }

        /// Every command's name.
        #[cfg(test)]
        const COMMANDS: &[&str] = &[$(commands!(@last $c $($d)*)),*];
    };
    (@last $c:ident) => { stringify!($c) };
    (@last $c:ident $($d:ident)+) => { commands!(@last $($d)+) };
}

commands![
    app_exe,
    changelog,
    set_badge,
    notify,
    menu::menu_keys,
    update::update_install,
    update::update_check,
    update::update_status,
    config::settings_current,
    config::settings_set,
    config::config_info,
    config::config_open,
    config::fonts_list,
    looks::theme_current,
    looks::theme_list,
    looks::theme_preview,
    looks::theme_choose,
    link::term_attach,
    link::term_release,
    link::term_input,
    link::term_resize,
    link::term_ack,
    link::term_request_full,
    link::layout_watch,
    link::layout_restore,
    link::layout_act,
    link::layout_looking,
    link::workspaces_list,
    link::workspace_open,
    link::workspace_save,
    screens::screen_current,
    screens::screen_list,
    screens::screen_load,
    screens::screen_choose,
    screens::part_exec,
    app_quit,
    log,
    open_link,
    dev_layout,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The command and event names, for the page.
    #[test]
    fn export_names() {
        let union = |all: &[&str]| {
            all.iter()
                .map(|n| format!("\n  | \"{n}\""))
                .collect::<String>()
        };
        let out = format!(
            "// Generated from app/src-tauri/src (lib.rs `commands!`, event.rs) by `cargo test`. Do not edit.\n\n\
             /** A command the app handles. */\nexport type Command ={};\n\n\
             /** An event the app sends. */\nexport type EventName ={};\n",
            union(COMMANDS),
            union(event::ALL),
        );
        let dir = std::env::var("TS_RS_EXPORT_DIR").expect("TS_RS_EXPORT_DIR");
        std::fs::write(std::path::Path::new(&dir).join("tauri.ts"), out).unwrap();
    }
}

/// `KIBANN_LAYOUT`: workspace actions to run at startup, comma separated
/// (`right`, `down`, `tab`). For trying layouts and capturing screenshots.
#[tauri::command]
fn dev_layout() -> Option<String> {
    std::env::var("KIBANN_LAYOUT").ok()
}

/// A line for the app's log (stderr) from the page.
#[tauri::command]
fn log(message: String) {
    eprintln!("kibann: {}", message.chars().take(500).collect::<String>());
}

/// Cmd+click on a link. `target` is a URL or an absolute path; see
/// [`open::plan`] for what is opened, revealed or ignored.
#[tauri::command]
fn open_link(app: AppHandle, target: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let opener = app.opener();
    let r = match open::plan(&target) {
        open::Open::Url(u) => opener.open_url(u, None::<&str>),
        open::Open::Path(p) => opener.open_path(p.to_string_lossy(), None::<&str>),
        open::Open::Reveal(p) => opener.reveal_item_in_dir(p),
        open::Open::Ignore => Ok(()),
    };
    r.map_err(|e| e.to_string())
}

/// The last pane closed: with the menu-bar icon, close the window and
/// keep running (showing it again opens a shell); without, quit.
#[tauri::command]
fn app_quit(app: AppHandle) {
    #[cfg(target_os = "macos")]
    if tray::enabled(&app) {
        tray::hide_window(&app);
        return;
    }
    app.exit(0);
}

/// What's New: the changelog this build was made with, and its version.
#[tauri::command]
fn changelog(app: AppHandle) -> (String, String) {
    (
        include_str!("../../../CHANGELOG.md").to_owned(),
        app.package_info().version.to_string(),
    )
}

/// The Dock badge: how many agents wait for the user (none: no badge).
#[tauri::command]
fn set_badge(window: tauri::WebviewWindow, count: u32) {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    tray::set_waiting(window.app_handle(), count);
    let r = window.set_badge_count((count > 0).then_some(i64::from(count)));
    if cfg!(debug_assertions) {
        eprintln!("kibann: badge {count}: {r:?}");
    }
}

/// A system notification (an agent waits or finished, in a pane the user
/// isn't looking at). Clicking it shows the pane: see [`notify`].
#[tauri::command]
fn notify(app: AppHandle, pane: u32, title: String, body: String) {
    notify::show(&app, pane, title, body);
}

/// This executable, which also runs `kibann` commands (the diff viewer).
#[tauri::command]
fn app_exe() -> Option<String> {
    std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.menu(menu_bar).on_menu_event(|app, event| {
        let id = event.id().0.as_str();
        if id.starts_with('{') {
            let _ = app.emit(event::MENU_ACTION, id);
        } else if id == QUIT_END_SESSIONS {
            // Asks first when shells are open.
            tray::quit(app);
        } else if id == QUIT_KEY {
            if tray::enabled(app) {
                tray::hide_window(app);
            } else {
                app.exit(0);
            }
        } else if id.starts_with(tray::PREFIX) {
            tray::on_menu(app, id);
        } else if id == WHATS_NEW {
            let _ = app.emit(event::WHATS_NEW, ());
        } else if id == CHECK_UPDATES {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { update::check(&app, true).await });
        }
    });
    // No app menu on Linux: the only menu is the tray icon's.
    #[cfg(target_os = "linux")]
    let builder = builder.on_menu_event(|app, event| {
        let id = event.id().0.as_str();
        if id.starts_with(tray::PREFIX) {
            tray::on_menu(app, id);
        }
    });
    // Opening the app again (from the launcher, with the window hidden in
    // the tray) shows the running one. It must be the first plugin.
    #[cfg(target_os = "linux")]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        tray::show_window(app);
    }));
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .register_asynchronous_uri_scheme_protocol(screens::SCHEME, |_ctx, request, responder| {
            std::thread::spawn(move || responder.respond(screens::respond(&request)));
        })
        .setup(|app| {
            let handle = app.handle();
            app.manage(Term::default());
            let term = app.state::<Term>();
            match service::connect(events(handle)) {
                Ok(service::Connected::Ready(c)) => term.set(c),
                Ok(service::Connected::Older(c, busy)) => {
                    term.set(c);
                    service::offer_update_restart(handle, &busy);
                }
                Ok(service::Connected::Unusable(busy)) => service::offer_restart(handle, &busy),
                Err(e) => {
                    eprintln!("kibann: no terminal service: {e}");
                    service::no_service(handle, &e);
                }
            }

            #[cfg(target_os = "macos")]
            if let Some(w) = app.get_webview_window("main") {
                titlebar::install(&w);
            }

            let dark = app
                .get_webview_window("main")
                .and_then(|w| w.theme().ok())
                .is_none_or(|t| t != tauri::Theme::Light);
            app.manage(Mutex::new(Looks {
                choice: Default::default(),
                dark,
                forced: None,
                preview: None,
                name: String::new(),
                theme: proto::theme::Theme::kibann_dark(),
            }));
            app.manage(Mutex::new(proto::config::Settings::default()));
            app.manage(Problems::default());
            app.manage(screens::Screens::default());
            reload_config(handle);
            watch_config(handle.clone());
            notify::init(handle);
            update::start(handle);
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            {
                let on = app
                    .state::<Mutex<proto::config::Settings>>()
                    .lock()
                    .unwrap()
                    .menu_bar_icon;
                if let Err(e) = tray::create(handle, on) {
                    eprintln!("kibann: menu-bar icon: {e}");
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // With the menu-bar icon, closing the window hides it: the app
            // keeps running (see tray.rs).
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if tray::enabled(window.app_handle()) {
                    api.prevent_close();
                    let _ = window.hide();
                }
                return;
            }
            if let tauri::WindowEvent::ThemeChanged(t) = event {
                let app = window.app_handle();
                {
                    let looks = app.state::<Mutex<Looks>>();
                    let mut l = looks.lock().unwrap();
                    // Held to the theme's appearance: this is our own change,
                    // not the system's.
                    if l.forced.is_some() {
                        return;
                    }
                    l.dark = *t != tauri::Theme::Light;
                }
                show_theme(app);
            }
        })
        .invoke_handler(handler())
        .build(tauri::generate_context!())
        .expect("failed to start the app")
        .run(|_app, _event| {
            // A click on the Dock icon with the window closed brings it back.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } = _event
            {
                tray::show_window(_app);
            }
        });
}
