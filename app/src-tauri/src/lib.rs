//! Tauri shell: one window, any number of terminal sessions (one per pane).
//!
//! Sessions and the layout live in the daemon; this file only turns the
//! frontend's commands into daemon requests, and frames and layouts into
//! channel messages and events.

mod open;

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

/// The last pane closed.
#[tauri::command]
fn app_quit(app: AppHandle) {
    app.exit(0);
}

/// The Dock badge: how many agents wait for the user (none: no badge).
#[tauri::command]
fn set_badge(window: tauri::WebviewWindow, count: u32) {
    let r = window.set_badge_count((count > 0).then_some(i64::from(count)));
    if cfg!(debug_assertions) {
        eprintln!("kibann: badge {count}: {r:?}");
    }
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
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .item(&MenuItemBuilder::with_id(QUIT_END_SESSIONS, "Quit and End Sessions").build(app)?)
        .quit()
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
    let edit = SubmenuBuilder::new(app, "Edit").copy().paste().build()?;
    let view = SubmenuBuilder::new(app, "View")
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
    MenuBuilder::new(app)
        .items(&[&app_menu, &shell, &edit, &view, &window])
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
            quit_ending_sessions(app);
        }
    });
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            set_badge,
            term_attach,
            term_release,
            term_input,
            term_resize,
            term_ack,
            term_request_full,
            layout_watch,
            layout_act,
            app_quit,
            open_link,
            dev_layout
        ])
        .run(tauri::generate_context!())
        .expect("failed to run the app");
}
