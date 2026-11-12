//! Tauri shell: one window, any number of terminal sessions (one per pane).
//!
//! Sessions live in `term`, which knows nothing about Tauri; this file only
//! turns the frontend's commands into session calls, and frames and exits
//! into channel messages and events.

mod open;

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use proto::{Frame, InputEvent, Resize};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};
use term::{Event, Session, Spawn};

type SessionId = u32;

#[derive(Default)]
struct Term {
    sessions: Mutex<BTreeMap<SessionId, Session>>,
    next_id: AtomicU32,
}

impl Term {
    fn with(&self, id: SessionId, f: impl FnOnce(&Session)) {
        if let Some(s) = self.sessions.lock().unwrap().get(&id) {
            f(s);
        }
    }
}

#[derive(Clone, Serialize)]
struct Exited {
    id: SessionId,
    code: Option<u32>,
}

/// Start a shell at `size`. With `cwd_from`, it starts in that session's
/// working directory (a new pane next to it).
// Commands are async so a busy session thread never blocks the main thread.
#[tauri::command]
async fn term_spawn(
    state: State<'_, Term>,
    size: Resize,
    cwd_from: Option<SessionId>,
) -> Result<SessionId, String> {
    // KIBANN_COMMAND runs a program instead of the login shell (split on
    // whitespace; for trying the app with vim, htop and the like).
    let argv = std::env::var("KIBANN_COMMAND")
        .ok()
        .map(|c| c.split_whitespace().map(String::from).collect());
    let mut sessions = state.sessions.lock().unwrap();
    let cwd = cwd_from.and_then(|id| sessions.get(&id)?.cwd());
    let session = Session::spawn(
        Spawn {
            argv,
            cwd,
            ..Spawn::default()
        },
        size,
    )
    .map_err(|e| format!("could not start the shell: {e}"))?;
    let id = state.next_id.fetch_add(1, Ordering::Relaxed) + 1;
    sessions.insert(id, session);
    Ok(id)
}

/// Sessions still running, for a reloaded page to reattach.
#[tauri::command]
async fn term_list(state: State<'_, Term>) -> Result<Vec<SessionId>, String> {
    Ok(state.sessions.lock().unwrap().keys().copied().collect())
}

/// Send `id`'s frames to `channel`, starting with a full frame. A reloaded
/// page attaches again.
#[tauri::command]
async fn term_attach(
    app: AppHandle,
    state: State<'_, Term>,
    id: SessionId,
    channel: Channel<Frame>,
) -> Result<(), String> {
    let sessions = state.sessions.lock().unwrap();
    let session = sessions.get(&id).ok_or("no such session")?;
    session.attach(move |event| match event {
        Event::Frame(frame) => {
            let _ = channel.send(frame);
        }
        // The frontend closes the pane; the session is dropped then.
        Event::Exited(code) => {
            let _ = app.emit("term-exited", Exited { id, code });
        }
    });
    Ok(())
}

/// Hang up on the shell and forget the session.
#[tauri::command]
async fn term_close(state: State<'_, Term>, id: SessionId) -> Result<(), String> {
    let session = state.sessions.lock().unwrap().remove(&id);
    // Dropping joins the session thread; do it outside the lock.
    drop(session);
    Ok(())
}

#[tauri::command]
async fn term_input(
    state: State<'_, Term>,
    id: SessionId,
    event: InputEvent,
) -> Result<(), String> {
    state.with(id, |s| s.input(event));
    Ok(())
}

#[tauri::command]
async fn term_resize(state: State<'_, Term>, id: SessionId, size: Resize) -> Result<(), String> {
    state.with(id, |s| s.resize(size));
    Ok(())
}

#[tauri::command]
async fn term_ack(state: State<'_, Term>, id: SessionId, seq: u64) -> Result<(), String> {
    state.with(id, |s| s.ack(seq));
    Ok(())
}

#[tauri::command]
async fn term_request_full(state: State<'_, Term>, id: SessionId) -> Result<(), String> {
    state.with(id, |s| s.request_full());
    Ok(())
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
        }
    });
    builder
        .plugin(tauri_plugin_opener::init())
        .manage(Term::default())
        .invoke_handler(tauri::generate_handler![
            term_spawn,
            term_list,
            term_attach,
            term_close,
            term_input,
            term_resize,
            term_ack,
            term_request_full,
            app_quit,
            open_link,
            dev_layout
        ])
        .build(tauri::generate_context!())
        .expect("failed to build the app")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // Dropping the sessions hangs up on the shells.
                if let Some(state) = app.try_state::<Term>() {
                    let all = std::mem::take(&mut *state.sessions.lock().unwrap());
                    drop(all);
                }
            }
        });
}
