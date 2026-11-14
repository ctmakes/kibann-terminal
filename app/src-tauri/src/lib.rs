//! Tauri shell: one window, any number of terminal sessions (one per pane).
//!
//! Sessions and the layout live in the daemon; this file only turns the
//! frontend's commands into daemon requests, and frames and layouts into
//! channel messages and events.

#[cfg(target_os = "macos")]
mod fonts;
mod open;
mod theme;
#[cfg(target_os = "macos")]
mod tray;
mod update;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use daemon::client::{Client, ClientEvent};
use proto::layout::{Action, Layout, PaneId, TabId};
use proto::wire::{ClientMsg, SessionId};
use proto::{InputEvent, Resize};
use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, State};

/// Where each attached session's frames go.
type Channels = Arc<Mutex<HashMap<SessionId, Channel>>>;

/// The connection to the daemon, which owns the sessions.
struct Term {
    client: Client,
    channels: Channels,
}

impl Term {
    fn send(&self, msg: ClientMsg) -> Result<(), String> {
        self.client.send(&msg).map_err(|e| e.to_string())
    }
}

/// Frames go to their session's channel as the JSON the daemon sent, without
/// being decoded here; layouts become `layout` events.
fn on_event(app: AppHandle, channels: Channels) -> impl FnMut(ClientEvent<'_>) + Send + 'static {
    move |event| match event {
        ClientEvent::Frame { session, json } => {
            if let Some(ch) = channels.lock().unwrap().get(&session) {
                let _ = ch.send(InvokeResponseBody::Json(json.to_owned()));
            }
        }
        ClientEvent::Layout(layout) => {
            let _ = app.emit("layout", layout);
        }
        ClientEvent::Status(panes) => {
            let _ = app.emit("agent-status", panes);
        }
        ClientEvent::Git(panes) => {
            let _ = app.emit("pane-git", panes);
        }
        // The daemon went away, and its shells with it: no panes are left.
        ClientEvent::Disconnected => {
            let _ = app.emit("layout", Layout::default());
            let _ = app.emit(
                "agent-status",
                std::collections::BTreeMap::<u32, proto::status::PaneStatus>::new(),
            );
            let _ = app.emit(
                "pane-git",
                std::collections::BTreeMap::<u32, proto::git::PaneDir>::new(),
            );
        }
    }
}

/// A running daemon this build can't talk to (a release build found one
/// from a version with another protocol).
#[cfg(unix)]
struct Outdated;

/// What to do with the daemon found on the socket.
#[derive(Debug, PartialEq, Eq)]
enum Found {
    Use,
    /// End it and start ours (dev builds only: it ends the sessions).
    Restart,
    /// Ask the user (release builds, protocol changed).
    Outdated,
}

fn decide(dev: bool, same_protocol: bool, same_build: bool, restarted: bool) -> Found {
    if restarted || (same_protocol && (same_build || !dev)) {
        Found::Use
    } else if dev {
        Found::Restart
    } else {
        Found::Outdated
    }
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

    #[test]
    fn which_daemons_are_kept() {
        // Release: an upgrade keeps the sessions unless the protocol changed.
        assert_eq!(decide(false, true, true, false), Found::Use);
        assert_eq!(decide(false, true, false, false), Found::Use);
        assert_eq!(decide(false, false, false, false), Found::Outdated);
        // Dev: any other build is replaced, once.
        assert_eq!(decide(true, true, true, false), Found::Use);
        assert_eq!(decide(true, true, false, false), Found::Restart);
        assert_eq!(decide(true, false, false, false), Found::Restart);
        assert_eq!(decide(true, true, false, true), Found::Use);
    }
}

/// Ask a daemon to end its sessions and exit, and wait until it has.
/// `Hello` and `Shutdown` keep their shape across protocol versions, so
/// this works on any daemon.
#[cfg(unix)]
fn stop_daemon(client: Client, socket: &std::path::Path) -> std::io::Result<()> {
    client.send(&ClientMsg::Shutdown)?;
    drop(client);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while socket.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Ok(())
}

/// Connect to the daemon, starting it if needed. The daemon is this same
/// executable run with `--daemon` (it costs about 1 MB more than a separate
/// binary, and nothing extra has to be packaged).
///
/// A daemon from another build: a dev build restarts it, so new terminal
/// code takes effect (ending dev sessions); a release build keeps using it
/// when the protocol matches, so upgrading doesn't end running sessions, and
/// otherwise reports it as [`Outdated`] for the user to decide.
#[cfg(unix)]
fn connect(app: &AppHandle, channels: &Channels) -> std::io::Result<Result<Client, Outdated>> {
    use proto::wire::PROTOCOL_VERSION;

    let dev = cfg!(debug_assertions);
    let socket = daemon::paths::socket_path(dev);
    let exe = std::env::current_exe()?;
    let command = vec![exe.clone().into_os_string(), "--daemon".into()];
    let expected = daemon::build_id(&exe).ok();
    let mut restarted = false;
    loop {
        let client =
            Client::connect_or_start(&socket, &command, on_event(app.clone(), channels.clone()))?;
        let (version, build) = client.hello()?;
        let same_build = Some(&build) == expected.as_ref();
        match decide(dev, version == PROTOCOL_VERSION, same_build, restarted) {
            Found::Use => return Ok(Ok(client)),
            Found::Outdated => return Ok(Err(Outdated)),
            Found::Restart => {
                eprintln!("kibann: restarting the daemon (build {build}, want {expected:?})");
                stop_daemon(client, &socket)?;
                restarted = true;
            }
        }
    }
}

/// An older daemon holds sessions this version can't show. Offer to end
/// them; meanwhile this run keeps its own sessions in the app.
#[cfg(unix)]
fn offer_restart(app: &AppHandle) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let handle = app.clone();
    app.dialog()
        .message(
            "The terminal service from an earlier version of Kibann Terminal is still \
             running your previous sessions, and this version can't show them.\n\n\
             Restart the service? This ends those sessions. Until then, new shells \
             end when you quit.",
        )
        .title("Restart the terminal service?")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Restart Service".into(),
            "Not Now".into(),
        ))
        .show(move |restart| {
            if !restart {
                return;
            }
            let socket = daemon::paths::socket_path(cfg!(debug_assertions));
            if let Ok(old) = Client::connect(&socket, |_| {}) {
                let _ = stop_daemon(old, &socket);
            }
            handle.restart();
        });
}

/// Send `id`'s frames to `channel`, starting with a full frame.
#[tauri::command]
async fn term_attach(
    state: State<'_, Term>,
    id: SessionId,
    channel: Channel,
) -> Result<(), String> {
    state.channels.lock().unwrap().insert(id, channel);
    state.send(ClientMsg::Attach { session: id })
}

/// The pane left the layout: stop sending its frames.
#[tauri::command]
async fn term_release(state: State<'_, Term>, id: SessionId) -> Result<(), String> {
    state.channels.lock().unwrap().remove(&id);
    state.send(ClientMsg::Detach { session: id })
}

#[tauri::command]
async fn term_input(
    state: State<'_, Term>,
    id: SessionId,
    event: InputEvent,
) -> Result<(), String> {
    state.send(ClientMsg::Input { session: id, event })
}

#[tauri::command]
async fn term_resize(state: State<'_, Term>, id: SessionId, size: Resize) -> Result<(), String> {
    state.send(ClientMsg::Resize { session: id, size })
}

#[tauri::command]
async fn term_ack(state: State<'_, Term>, id: SessionId, seq: u64) -> Result<(), String> {
    state.send(ClientMsg::Ack { session: id, seq })
}

#[tauri::command]
async fn term_request_full(state: State<'_, Term>, id: SessionId) -> Result<(), String> {
    state.send(ClientMsg::RequestFull { session: id })
}

/// The layout now. It also arrives as the first `layout` event, and every
/// change after it.
#[tauri::command]
async fn layout_watch(state: State<'_, Term>) -> Result<Layout, String> {
    state.client.watch().map_err(|e| e.to_string())
}

/// With no panes open: reopen the saved tabs and splits, with fresh shells.
/// Returns how many panes came back (0: none).
#[tauri::command]
async fn layout_restore(state: State<'_, Term>) -> Result<u32, String> {
    state
        .client
        .restore()
        .map(|(_, panes)| panes)
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct Acted {
    pane: Option<PaneId>,
    tab: Option<TabId>,
}

/// Change the layout (a shortcut, a menu item, a click).
#[tauri::command]
async fn layout_act(state: State<'_, Term>, action: Action) -> Result<Acted, String> {
    // KIBANN_COMMAND runs a program instead of the login shell (split on
    // whitespace; for trying the app with vim, htop and the like).
    let command = || {
        std::env::var("KIBANN_COMMAND")
            .ok()
            .map(|c| c.split_whitespace().map(String::from).collect())
    };
    let action = match action {
        Action::NewTab {
            near,
            cwd,
            argv: None,
            focus,
        } => Action::NewTab {
            near,
            cwd,
            argv: command(),
            focus,
        },
        Action::Split {
            pane,
            dir,
            cwd,
            argv: None,
            focus,
        } => Action::Split {
            pane,
            dir,
            cwd,
            argv: command(),
            focus,
        },
        other => other,
    };
    // Dev builds log every action, to see what a shortcut or click sent.
    if cfg!(debug_assertions) {
        eprintln!("kibann: layout action {action:?}");
    }
    let (pane, tab) = state.client.act(action).map_err(|e| e.to_string())?;
    Ok(Acted { pane, tab })
}

/// `KIBANN_LAYOUT`: workspace actions to run at startup, comma separated
/// (`right`, `down`, `tab`). For trying layouts and capturing screenshots.
#[tauri::command]
fn dev_layout() -> Option<String> {
    std::env::var("KIBANN_LAYOUT").ok()
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
    #[cfg(target_os = "macos")]
    tray::set_waiting(window.app_handle(), count);
    let r = window.set_badge_count((count > 0).then_some(i64::from(count)));
    if cfg!(debug_assertions) {
        eprintln!("kibann: badge {count}: {r:?}");
    }
}

// ---- themes ----

/// Which theme is in use, and why.
struct Looks {
    /// From the config file.
    choice: proto::theme::ThemeChoice,
    /// The system appearance.
    dark: bool,
    /// The appearance the window is held to (the theme's), or `None` when
    /// it follows the system.
    forced: Option<bool>,
    /// Shown by the picker, not kept yet.
    preview: Option<String>,
    /// In use now.
    name: String,
    theme: proto::theme::Theme,
}

#[derive(Clone, Serialize)]
struct ThemeNow {
    name: String,
    theme: proto::theme::Theme,
}

/// Show the right theme (the preview, else the config's for the
/// appearance): tell the daemon, which colors the terminals, and the
/// frontend, which colors the window. A theme that can't be loaded falls
/// back to the built-in one, with a notice.
fn show_theme(app: &AppHandle) {
    let looks = app.state::<Mutex<Looks>>();
    let (name, one) = {
        let l = looks.lock().unwrap();
        let name = l
            .preview
            .clone()
            .unwrap_or_else(|| l.choice.for_appearance(l.dark).to_owned());
        // One theme for both appearances (or a preview): the window takes
        // the theme's. With a light/dark pair the theme already matches
        // the system, which keeps switching it.
        (name, l.preview.is_some() || l.choice.light == l.choice.dark)
    };
    let (name, theme) = match theme::load(&name) {
        Ok((t, errors)) => {
            for e in errors {
                let _ = app.emit("notice", format!("Theme \"{name}\": {e}"));
            }
            (name, t)
        }
        Err(e) => {
            let _ = app.emit("notice", format!("{e}; using the built-in theme"));
            let dark = looks.lock().unwrap().dark;
            let n = if dark {
                proto::theme::DEFAULT_DARK
            } else {
                proto::theme::DEFAULT_LIGHT
            };
            (n.to_owned(), theme::load(n).expect("built in").0)
        }
    };
    window_colors(app, &theme, one.then(|| theme.is_dark()));
    {
        let mut l = looks.lock().unwrap();
        if l.name == name && l.theme == theme {
            return;
        }
        l.name.clone_from(&name);
        l.theme = theme.clone();
    }
    if let Some(term) = app.try_state::<Term>() {
        let _ = term.client.request(|req| ClientMsg::SetTheme {
            req,
            theme: theme.clone(),
        });
    }
    let _ = app.emit("theme", ThemeNow { name, theme });
}

/// The window around the webview in the theme's colors: the title bar's
/// light or dark appearance, and on macOS (a transparent title bar, see
/// `tauri.conf.json`) the strip behind the title in the tab bar's color.
fn window_colors(app: &AppHandle, theme: &proto::theme::Theme, force: Option<bool>) {
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    let changed = {
        let looks = app.state::<Mutex<Looks>>();
        let mut l = looks.lock().unwrap();
        std::mem::replace(&mut l.forced, force) != force
    };
    if changed {
        let _ = w.set_theme(force.map(|d| {
            if d {
                tauri::Theme::Dark
            } else {
                tauri::Theme::Light
            }
        }));
        // Following the system again: it may have changed meanwhile.
        if force.is_none()
            && let Ok(t) = w.theme()
        {
            let dark = t != tauri::Theme::Light;
            let looks = app.state::<Mutex<Looks>>();
            let was = std::mem::replace(&mut looks.lock().unwrap().dark, dark);
            if was != dark {
                show_theme(app);
                return;
            }
        }
    }
    // The tab bar's color (`--k-bar` in app/src/workspace/theme.ts).
    let bar = proto::theme::mix(theme.background, theme.foreground, 0.067);
    let [_, r, g, b] = bar.to_be_bytes();
    let _ = w.set_background_color(Some(tauri::window::Color(r, g, b, 255)));
}

/// Read the config file again (it changed, or at start).
fn reload_choice(app: &AppHandle) {
    let (choice, errors) = theme::read_choice();
    let (settings, more) = theme::read_settings();
    let problems: Vec<String> = errors.into_iter().chain(more).collect();
    for e in &problems {
        let _ = app.emit("notice", format!("Config: {e}"));
    }
    *app.state::<Problems>().0.lock().unwrap() = problems;
    app.state::<Mutex<Looks>>().lock().unwrap().choice = choice;
    // Settings, if open, shows the file's problems and themes.
    let _ = app.emit("config-info", config_info(app.clone()));
    show_theme(app);
    let changed = {
        let state = app.state::<Mutex<proto::config::Settings>>();
        let mut now = state.lock().unwrap();
        let changed = *now != settings;
        *now = settings.clone();
        changed
    };
    if changed {
        #[cfg(target_os = "macos")]
        {
            tray::set_visible(app, settings.menu_bar_icon);
            set_quit_labels(app, settings.menu_bar_icon);
        }
        let _ = app.emit("settings", settings);
    }
}

/// The config file's lines that couldn't be used, as of its last read.
#[derive(Default)]
struct Problems(Mutex<Vec<String>>);

/// What Settings shows about the config file.
#[derive(Clone, Serialize)]
struct ConfigInfo {
    path: String,
    problems: Vec<String>,
    /// The themes chosen for each appearance.
    light: String,
    dark: String,
}

#[tauri::command]
fn config_info(app: AppHandle) -> ConfigInfo {
    let choice = app.state::<Mutex<Looks>>().lock().unwrap().choice.clone();
    ConfigInfo {
        path: theme::config_file()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        problems: app.state::<Problems>().0.lock().unwrap().clone(),
        light: choice.light,
        dark: choice.dark,
    }
}

/// Settings changed a setting: write it to the config file, where the
/// watcher picks it up as it would an edit by hand.
#[tauri::command]
fn settings_set(key: String, values: Vec<String>) -> Result<(), String> {
    theme::save_values(&key, &values)
}

/// Open the config file in the text editor, or show it in the file
/// manager (`reveal`); made first if it doesn't exist.
#[tauri::command]
fn config_open(app: AppHandle, reveal: bool) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let file = theme::config_file().ok_or("no home directory")?;
    if !file.exists() {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(
            &file,
            "# Kibann Terminal's settings, in Ghostty's syntax. They apply when saved.\n\
             # See https://github.com/ctmakes/kibann-terminal#configuration\n",
        )
        .map_err(|e| e.to_string())?;
    }
    if reveal {
        return app
            .opener()
            .reveal_item_in_dir(&file)
            .map_err(|e| e.to_string());
    }
    // The text editor, not whatever opens files without an extension.
    #[cfg(target_os = "macos")]
    return std::process::Command::new("/usr/bin/open")
        .arg("-t")
        .arg(&file)
        .status()
        .map_err(|e| e.to_string())
        .and_then(|s| {
            s.success()
                .then_some(())
                .ok_or_else(|| format!("open: {s}"))
        });
    #[cfg(not(target_os = "macos"))]
    app.opener()
        .open_path(file.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// The installed monospaced fonts, or `None` where they can't be listed
/// (Settings then takes a typed name).
#[tauri::command]
async fn fonts_list() -> Option<Vec<String>> {
    #[cfg(target_os = "macos")]
    return Some(fonts::monospaced());
    #[cfg(not(target_os = "macos"))]
    None
}

#[tauri::command]
fn settings_current(
    settings: State<'_, Mutex<proto::config::Settings>>,
) -> proto::config::Settings {
    settings.lock().unwrap().clone()
}

/// A system notification (an agent waits or finished, in a pane the user
/// isn't looking at). Clicking it shows the pane: see [`on_notification_click`].
#[tauri::command]
fn notify(app: AppHandle, pane: u32, title: String, body: String) {
    use tauri_plugin_notification::NotificationExt;
    let r = app
        .notification()
        .builder()
        .id(i32::try_from(pane).unwrap_or(0))
        .title(title)
        .body(body)
        .show();
    if let Err(e) = r {
        eprintln!("kibann: notification: {e}");
    }
}

/// The shortcuts changed (the config's `keybind`): show each menu item's
/// new key, or none, so a key the config moved elsewhere doesn't also run
/// the menu item it used to. `keys` maps the items' ids (actions as JSON).
#[tauri::command]
fn menu_keys(app: AppHandle, keys: std::collections::HashMap<String, Option<String>>) {
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

/// A click on one of our notifications: bring the window forward and show
/// the pane it's about (its id is the notification's). A pane closed since
/// does nothing.
fn on_notification_click(app: &AppHandle) {
    use tauri_plugin_notification::NotificationExt;
    let handle = app.clone();
    let _ = app.notification().on_action(move |done| {
        if done.action_id() != "tap" {
            return;
        }
        let Some(pane) = done.notification().map(|n| n.id()) else {
            return;
        };
        #[cfg(target_os = "macos")]
        tray::show_window(&handle);
        #[cfg(not(target_os = "macos"))]
        if let Some(w) = handle.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
        let _ = handle.emit("focus-pane", pane);
    });
}

/// Poll the config file and the user's themes: editors save in ways
/// (rename over) that file watchers report unreliably, and once a second
/// costs nothing.
fn watch_config(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("config-watch".into())
        .spawn(move || {
            let mut last = theme::stamp();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let now = theme::stamp();
                if now != last {
                    last = now;
                    reload_choice(&app);
                }
            }
        });
}

#[tauri::command]
fn theme_current(looks: State<'_, Mutex<Looks>>) -> ThemeNow {
    let l = looks.lock().unwrap();
    ThemeNow {
        name: l.name.clone(),
        theme: l.theme.clone(),
    }
}

#[tauri::command]
async fn theme_list() -> Vec<theme::Entry> {
    theme::list()
}

/// Show `name` for now; `None` goes back to the kept theme.
#[tauri::command]
async fn theme_preview(app: AppHandle, name: Option<String>) {
    app.state::<Mutex<Looks>>().lock().unwrap().preview = name;
    show_theme(&app);
}

/// Keep `name` for the current appearance, in the config file.
#[tauri::command]
async fn theme_choose(app: AppHandle, name: String) -> Result<(), String> {
    let choice = {
        let looks = app.state::<Mutex<Looks>>();
        let mut l = looks.lock().unwrap();
        l.preview = None;
        if l.dark {
            l.choice.dark.clone_from(&name);
        } else {
            l.choice.light.clone_from(&name);
        }
        l.choice.clone()
    };
    let saved = theme::save_choice(&choice);
    show_theme(&app);
    saved
}

/// This executable, which also runs `kibann` commands (the diff viewer).
#[tauri::command]
fn app_exe() -> Option<String> {
    std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// End every shell and the daemon, then quit.
#[cfg(target_os = "macos")]
fn quit_ending_sessions(app: &AppHandle) {
    if let Some(t) = app.try_state::<Term>() {
        let _ = t.send(ClientMsg::Shutdown);
    }
    app.exit(0);
}

/// Menu item id for [`quit_ending_sessions`].
#[cfg(target_os = "macos")]
const QUIT_END_SESSIONS: &str = "quit-end-sessions";
/// Menu item id for Cmd+Q: with the menu-bar icon it only closes the window
/// (the app keeps watching agents), without it it quits the app.
#[cfg(target_os = "macos")]
const QUIT_KEY: &str = "quit-key";

/// The app menu's two quits say what they do, which depends on whether the
/// app keeps running in the menu bar.
#[cfg(target_os = "macos")]
fn quit_labels(menu_bar_icon: bool) -> [(&'static str, &'static str); 2] {
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
fn set_quit_labels(app: &AppHandle, menu_bar_icon: bool) {
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
const CHECK_UPDATES: &str = "check-updates";
/// Menu item id for Help → What's New.
#[cfg(target_os = "macos")]
const WHATS_NEW: &str = "whats-new";

/// The macOS menu bar: the standard menus plus the workspace actions with
/// their shortcuts. An item's id is the frontend action as JSON.
#[cfg(target_os = "macos")]
fn menu_bar(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
    let item = |action: &str, text: &str, accel: &str| {
        MenuItemBuilder::with_id(action, text)
            .accelerator(accel)
            .build(app)
    };
    let app_menu = SubmenuBuilder::new(app, "Kibann Terminal")
        .about(None)
        .separator()
        .item(&item(r#"{"kind":"settings"}"#, "Settings…", "Cmd+Comma")?)
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
        .item(&item(r#"{"kind":"newTab"}"#, "New Tab", "Cmd+T")?)
        .separator()
        .item(&item(
            r#"{"kind":"split","dir":"row"}"#,
            "Split Right",
            "Cmd+D",
        )?)
        .item(&item(
            r#"{"kind":"split","dir":"column"}"#,
            "Split Down",
            "Shift+Cmd+D",
        )?)
        .separator()
        .item(&item(r#"{"kind":"close"}"#, "Close", "Cmd+W")?)
        .build()?;
    let edit = SubmenuBuilder::new(app, "Edit")
        .copy()
        .paste()
        .separator()
        .item(&item(r#"{"kind":"find"}"#, "Find…", "Cmd+F")?)
        .item(&item(
            r#"{"kind":"findStep","older":true}"#,
            "Find Next",
            "Cmd+G",
        )?)
        .item(&item(
            r#"{"kind":"findStep","older":false}"#,
            "Find Previous",
            "Shift+Cmd+G",
        )?)
        .build()?;
    let view = SubmenuBuilder::new(app, "View")
        .item(&item(
            r#"{"kind":"sidebar"}"#,
            "Toggle Sidebar",
            "Shift+Cmd+B",
        )?)
        .item(&MenuItemBuilder::with_id(r#"{"kind":"themes"}"#, "Theme…").build(app)?)
        .separator()
        .item(&item(
            r#"{"kind":"fontSize","step":1}"#,
            "Bigger",
            "Cmd+Equal",
        )?)
        .item(&item(
            r#"{"kind":"fontSize","step":-1}"#,
            "Smaller",
            "Cmd+Minus",
        )?)
        .item(&item(
            r#"{"kind":"fontSize","step":0}"#,
            "Actual Size",
            "Cmd+Digit0",
        )?)
        .separator()
        .item(&item(
            r#"{"kind":"zoom"}"#,
            "Zoom Split",
            "Shift+Cmd+Enter",
        )?)
        .item(&item(
            r#"{"kind":"equalize"}"#,
            "Equalize Splits",
            "Ctrl+Cmd+=",
        )?)
        .separator()
        .fullscreen()
        .build()?;
    let window = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .separator()
        .item(&item(
            r#"{"kind":"tabCycle","step":-1}"#,
            "Show Previous Tab",
            "Shift+Cmd+[",
        )?)
        .item(&item(
            r#"{"kind":"tabCycle","step":1}"#,
            "Show Next Tab",
            "Shift+Cmd+]",
        )?)
        .separator()
        .item(&item(
            r#"{"kind":"focusCycle","step":-1}"#,
            "Select Previous Split",
            "Cmd+[",
        )?)
        .item(&item(
            r#"{"kind":"focusCycle","step":1}"#,
            "Select Next Split",
            "Cmd+]",
        )?)
        .build()?;
    let help = SubmenuBuilder::new(app, "Help")
        .item(&item(
            r#"{"kind":"shortcuts"}"#,
            "Keyboard Shortcuts",
            "Cmd+/",
        )?)
        .item(&MenuItemBuilder::with_id(WHATS_NEW, "What's New").build(app)?)
        .build()?;
    MenuBuilder::new(app)
        .items(&[&app_menu, &shell, &edit, &view, &window, &help])
        .build()
}

pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.menu(menu_bar).on_menu_event(|app, event| {
        let id = event.id().0.as_str();
        if id.starts_with('{') {
            let _ = app.emit("menu-action", id);
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
            let _ = app.emit("whats-new", ());
        } else if id == CHECK_UPDATES {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { update::check(&app, true).await });
        }
    });
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let channels = Channels::default();
            let handle = app.handle();
            // Without the daemon the app still works; sessions just end with
            // it, as before.
            let local = || Client::in_process(on_event(handle.clone(), channels.clone()));
            #[cfg(unix)]
            let client = match connect(handle, &channels) {
                Ok(Ok(client)) => client,
                Ok(Err(Outdated)) => {
                    offer_restart(handle);
                    local()?
                }
                Err(e) => {
                    eprintln!("kibann: no daemon ({e}); running sessions in the app");
                    local()?
                }
            };
            #[cfg(not(unix))]
            let client = local()?;
            app.manage(Term { client, channels });

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
            reload_choice(handle);
            watch_config(handle.clone());
            on_notification_click(handle);
            update::start(handle);
            #[cfg(target_os = "macos")]
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
            #[cfg(target_os = "macos")]
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
        .invoke_handler(tauri::generate_handler![
            app_exe,
            changelog,
            set_badge,
            notify,
            menu_keys,
            update::update_install,
            update::update_check,
            update::update_status,
            settings_current,
            settings_set,
            config_info,
            config_open,
            fonts_list,
            theme_current,
            theme_list,
            theme_preview,
            theme_choose,
            term_attach,
            term_release,
            term_input,
            term_resize,
            term_ack,
            term_request_full,
            layout_watch,
            layout_restore,
            layout_act,
            app_quit,
            open_link,
            dev_layout
        ])
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
