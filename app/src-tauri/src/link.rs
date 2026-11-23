//! The link to the daemon: the one connection, which pane's channel each
//! frame goes to, reconnecting when the service ends, and the commands that
//! turn the page's requests into daemon requests.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use client::{Client, ClientEvent};
use proto::layout::{Action, Layout, PaneId, TabId};
use proto::wire::ClientMsg;
use proto::{InputEvent, Resize};
use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::looks::send_theme;
use crate::{event, service};

/// Where each attached session's frames go.
pub(crate) type Channels = Arc<Mutex<HashMap<PaneId, Channel>>>;

/// The connection to the daemon, which owns the sessions. None while there
/// is no service to use (see [`service`]).
#[derive(Default)]
pub(crate) struct Term {
    pub(crate) link: std::sync::RwLock<Option<Arc<Client>>>,
    pub(crate) channels: Channels,
    /// Connections made so far: each one's events know its number.
    pub(crate) made: AtomicU64,
    /// The number of the connection in use, 0 for none. Only its end means
    /// the service went away (not a replaced service's, nor a failed try's).
    pub(crate) live: AtomicU64,
}

impl Term {
    pub(crate) fn client(&self) -> Result<Arc<Client>, String> {
        self.link
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| "no terminal service".to_owned())
    }

    /// Use `client`, the connection [`events`] was last asked to handle.
    pub(crate) fn set(&self, client: Client) {
        *self.link.write().unwrap() = Some(Arc::new(client));
        self.live
            .store(self.made.load(Ordering::SeqCst), Ordering::SeqCst);
    }

    pub(crate) fn send(&self, msg: ClientMsg) -> Result<(), String> {
        self.client()?.send(&msg).map_err(|e| e.to_string())
    }
}

/// The handler for each new connection's events (see [`on_event`]).
pub(crate) fn events(
    app: &AppHandle,
) -> impl Fn() -> Box<dyn FnMut(ClientEvent<'_>) + Send> + use<> {
    let app = app.clone();
    move || {
        let term = app.state::<Term>();
        let me = term.made.fetch_add(1, Ordering::SeqCst) + 1;
        Box::new(on_event(app.clone(), term.channels.clone(), me))
    }
}

/// Frames go to their session's channel as the JSON the daemon sent, without
/// being decoded here; layouts become `layout` events. `me`: this
/// connection's number (see [`Term::live`]).
pub(crate) fn on_event(
    app: AppHandle,
    channels: Channels,
    me: u64,
) -> impl FnMut(ClientEvent<'_>) + Send + 'static {
    move |event| match event {
        ClientEvent::Frame { session, json } => {
            if let Some(ch) = channels.lock().unwrap().get(&session) {
                let _ = ch.send(InvokeResponseBody::Json(json.to_owned()));
            }
        }
        ClientEvent::Layout(layout) => {
            let _ = app.emit(event::LAYOUT, layout);
        }
        ClientEvent::Status(panes) => {
            let _ = app.emit(event::AGENT_STATUS, panes);
        }
        ClientEvent::Git(panes) => {
            let _ = app.emit(event::PANE_GIT, panes);
        }
        // The daemon went away, and its shells with it: no panes are left.
        // The page clears them (without quitting, as when the last pane
        // closes), and a new service starts.
        ClientEvent::Disconnected => {
            let term = app.state::<Term>();
            if term
                .live
                .compare_exchange(me, 0, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                return;
            }
            *term.link.write().unwrap() = None;
            let _ = app.emit(event::SERVICE_LOST, ());
            let _ = app.emit(
                event::AGENT_STATUS,
                std::collections::BTreeMap::<u32, proto::status::PaneStatus>::new(),
            );
            let _ = app.emit(
                event::PANE_GIT,
                std::collections::BTreeMap::<u32, proto::git::PaneDir>::new(),
            );
            reconnect(app.clone());
        }
    }
}

/// Connect to a service again, starting one, sooner at first and then
/// every few seconds. Then bring back the saved tabs (or open one), as at
/// launch, and tell the page (`service-back`, with the panes reopened).
pub(crate) fn reconnect(app: AppHandle) {
    std::thread::spawn(move || {
        let mut wait = Duration::from_millis(200);
        loop {
            std::thread::sleep(wait);
            let c = match service::connect(events(&app)) {
                Ok(service::Connected::Ready(c) | service::Connected::Older(c, _)) => c,
                Ok(service::Connected::Unusable(busy)) => {
                    service::offer_restart(&app, &busy);
                    return;
                }
                Err(e) => {
                    eprintln!("kibann: no terminal service yet: {e}");
                    wait = (wait * 2).min(Duration::from_secs(5));
                    continue;
                }
            };
            send_theme(&app, &c);
            let restored = reopen(&c);
            app.state::<Term>().set(c);
            let _ = app.emit(event::SERVICE_BACK, restored);
            return;
        }
    });
}

/// With no panes open, the saved tabs and splits (if restore-layout is on),
/// else one new tab; then watch the layout. How many panes came back.
pub(crate) fn reopen(c: &Client) -> u32 {
    let restored = if files::config::read_settings().0.restore_layout {
        c.restore().map_or(0, |(_, panes)| panes)
    } else {
        0
    };
    if let Ok(layout) = c.watch()
        && layout.tabs.is_empty()
    {
        let _ = c.act(Action::NewTab {
            near: None,
            cwd: None,
            argv: None,
            focus: true,
        });
    }
    restored
}

/// Send `id`'s frames to `channel`, starting with a full frame.
#[tauri::command]
pub async fn term_attach(
    state: State<'_, Term>,
    id: PaneId,
    channel: Channel,
) -> Result<(), String> {
    state.channels.lock().unwrap().insert(id, channel);
    state.send(ClientMsg::Attach { session: id })
}

/// The pane left the layout: stop sending its frames.
#[tauri::command]
pub async fn term_release(state: State<'_, Term>, id: PaneId) -> Result<(), String> {
    state.channels.lock().unwrap().remove(&id);
    state.send(ClientMsg::Detach { session: id })
}

#[tauri::command]
pub async fn term_input(
    state: State<'_, Term>,
    id: PaneId,
    event: InputEvent,
) -> Result<(), String> {
    state.send(ClientMsg::Input { session: id, event })
}

#[tauri::command]
pub async fn term_resize(state: State<'_, Term>, id: PaneId, size: Resize) -> Result<(), String> {
    state.send(ClientMsg::Resize { session: id, size })
}

#[tauri::command]
pub async fn term_ack(state: State<'_, Term>, id: PaneId, seq: u64) -> Result<(), String> {
    state.send(ClientMsg::Ack { session: id, seq })
}

#[tauri::command]
pub async fn term_request_full(state: State<'_, Term>, id: PaneId) -> Result<(), String> {
    state.send(ClientMsg::RequestFull { session: id })
}

/// The layout now. It also arrives as the first `layout` event, and every
/// change after it.
#[tauri::command]
pub async fn layout_watch(state: State<'_, Term>) -> Result<Layout, String> {
    state.client()?.watch().map_err(|e| e.to_string())
}

/// The pane the user is looking at in this window, or none (it doesn't
/// have focus): agents finishing there aren't "done".
#[tauri::command]
pub fn layout_looking(
    state: State<'_, Term>,
    pane: Option<proto::layout::PaneId>,
) -> Result<(), String> {
    state.client()?.looking(pane).map_err(|e| e.to_string())
}

/// With no panes open: reopen the saved tabs and splits, with fresh shells.
/// Returns how many panes came back (0: none).
#[tauri::command]
pub async fn layout_restore(state: State<'_, Term>) -> Result<u32, String> {
    state
        .client()?
        .restore()
        .map(|(_, panes)| panes)
        .map_err(|e| e.to_string())
}

/// The pane and tab an action made or acted on.
#[derive(Serialize, ts_rs::TS)]
#[ts(export)]
pub(crate) struct Acted {
    pub(crate) pane: Option<PaneId>,
    pub(crate) tab: Option<TabId>,
}

/// Change the layout (a shortcut, a menu item, a click).
#[tauri::command]
pub async fn layout_act(state: State<'_, Term>, action: Action) -> Result<Acted, String> {
    // Dev builds log every action, to see what a shortcut or click sent.
    if cfg!(debug_assertions) {
        eprintln!("kibann: layout action {action:?}");
    }
    let (pane, tab) = state.client()?.act(action).map_err(|e| e.to_string())?;
    Ok(Acted { pane, tab })
}

/// Where a layout is opened from: `pane`'s directory, else home.
pub(crate) fn opened_from(client: &Client, pane: Option<PaneId>) -> std::path::PathBuf {
    let cwd = pane.and_then(|id| match client.request(|req| ClientMsg::Panes { req }) {
        Ok(proto::wire::Reply::Panes { panes }) => {
            panes.into_iter().find(|p| p.id == id).and_then(|p| p.cwd)
        }
        _ => None,
    });
    cwd.map(std::path::PathBuf::from)
        .filter(|d| d.is_dir())
        .or_else(proto::paths::home_dir)
        .unwrap_or_else(|| std::path::PathBuf::from("/"))
}

/// The workspaces for the focused pane's directory (View → Open Workspace…).
#[tauri::command]
pub async fn workspaces_list(
    state: State<'_, Term>,
    pane: Option<PaneId>,
) -> Result<Vec<files::workspaces::Listed>, String> {
    Ok(files::workspaces::list(&opened_from(
        &*state.client()?,
        pane,
    )))
}

/// Open a workspace: a built-in one by `name`, a file by `path`. A file that
/// runs commands from someone else asks first; false when the user says no.
#[tauri::command]
pub async fn workspace_open(
    app: AppHandle,
    state: State<'_, Term>,
    name: String,
    path: Option<String>,
    pane: Option<PaneId>,
) -> Result<bool, String> {
    use files::workspaces;
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let here = opened_from(&*state.client()?, pane);
    let f = workspaces::find(Some(path.as_deref().unwrap_or(&name)), &here)?;
    let t = workspaces::parse(&f, &here).map_err(|p| {
        p.0.iter()
            .map(|p| format!("line {}: {}", p.line, p.message))
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    if workspaces::needs_asking(&f, &t) {
        let file = f
            .path
            .as_ref()
            .map_or_else(|| f.name.clone(), |p| p.display().to_string());
        let list: Vec<String> = workspaces::commands(&t)
            .into_iter()
            .map(|r| format!("• {}", r.run))
            .collect();
        let yes = app
            .dialog()
            .message(format!(
                "{file} runs these commands:\n\n{}\n\nOnly open workspaces from people you trust.",
                list.join("\n")
            ))
            .title(format!("Open the workspace {}?", f.name))
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Open and Run".into(),
                "Cancel".into(),
            ))
            .blocking_show();
        if !yes {
            return Ok(false);
        }
        if let Some(p) = &f.path {
            workspaces::trust(p, &f.text).map_err(|e| e.to_string())?;
        }
    }
    state
        .client()?
        .act(Action::OpenWorkspace { workspace: t })
        .map_err(|e| e.to_string())?;
    Ok(true)
}

/// Save the window as the user's workspace `name` (View → Save Workspace…).
/// Returns where; fails saying it "exists" unless `force`.
#[tauri::command]
pub async fn workspace_save(
    state: State<'_, Term>,
    name: String,
    force: bool,
) -> Result<String, String> {
    use files::workspaces;
    if name.is_empty() || name.starts_with('.') || name.contains(['/', '\\']) {
        return Err(format!("{name:?} can't be a workspace's name"));
    }
    let dir = proto::paths::workspaces_dir().ok_or("no home directory")?;
    let target = dir.join(format!("{name}.toml"));
    if target.exists() && !force {
        return Err(format!("{} exists", target.display()));
    }
    let layout = state.client()?.layout().map_err(|e| e.to_string())?;
    let panes = match state.client()?.request(|req| ClientMsg::Panes { req }) {
        Ok(proto::wire::Reply::Panes { panes }) => panes,
        Ok(other) => return Err(format!("unexpected reply: {other:?}")),
        Err(e) => return Err(e.to_string()),
    };
    let text = workspaces::snapshot(
        &layout,
        &panes,
        &workspaces::SaveSpec {
            name: Some(&name),
            tab: None,
            relative_to: None,
        },
    )?;
    workspaces::save(&target, &text).map_err(|e| e.to_string())?;
    Ok(target.display().to_string())
}
