//! Tauri shell: one window, one terminal session.
//!
//! The session lives in `term`, which knows nothing about Tauri; this file
//! only turns the frontend's commands into session calls and frames into
//! channel messages.

use std::sync::Mutex;

use proto::{Frame, InputEvent, Resize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use term::{Event, Session, Spawn};

/// Size used if the frontend attaches before telling us its size.
const DEFAULT_SIZE: Resize = Resize {
    cols: 80,
    rows: 24,
    cell_w: 8,
    cell_h: 16,
};

#[derive(Default)]
struct Term {
    session: Mutex<Option<Session>>,
    /// The latest size from the frontend, used when the shell starts.
    size: Mutex<Option<Resize>>,
}

fn with_session(state: &Term, f: impl FnOnce(&Session)) {
    if let Some(s) = state.session.lock().unwrap().as_ref() {
        f(s);
    }
}

/// Start sending frames to `channel`, starting the shell on first use. A
/// reloaded page attaches again and gets a full frame.
// Commands are async so a busy session thread never blocks the main thread.
#[tauri::command]
async fn term_attach(
    app: AppHandle,
    state: State<'_, Term>,
    channel: Channel<Frame>,
) -> Result<(), String> {
    let mut session = state.session.lock().unwrap();
    if session.is_none() {
        let size = state.size.lock().unwrap().unwrap_or(DEFAULT_SIZE);
        // KIBANN_COMMAND runs a program instead of the login shell (split on
        // whitespace; for trying the app with vim, htop and the like).
        let argv = std::env::var("KIBANN_COMMAND")
            .ok()
            .map(|c| c.split_whitespace().map(String::from).collect());
        *session = Some(
            Session::spawn(
                Spawn {
                    argv,
                    ..Spawn::default()
                },
                size,
            )
            .map_err(|e| format!("could not start the shell: {e}"))?,
        );
    }
    session.as_ref().unwrap().attach(move |event| match event {
        Event::Frame(frame) => {
            let _ = channel.send(frame);
        }
        // Like other terminals: the window goes away with its shell.
        Event::Exited(_) => app.exit(0),
    });
    Ok(())
}

#[tauri::command]
async fn term_input(state: State<'_, Term>, event: InputEvent) -> Result<(), String> {
    with_session(&state, |s| s.input(event));
    Ok(())
}

#[tauri::command]
async fn term_resize(state: State<'_, Term>, size: Resize) -> Result<(), String> {
    *state.size.lock().unwrap() = Some(size);
    with_session(&state, |s| s.resize(size));
    Ok(())
}

#[tauri::command]
async fn term_ack(state: State<'_, Term>, seq: u64) -> Result<(), String> {
    with_session(&state, |s| s.ack(seq));
    Ok(())
}

#[tauri::command]
async fn term_request_full(state: State<'_, Term>) -> Result<(), String> {
    with_session(&state, |s| s.request_full());
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .manage(Term::default())
        .invoke_handler(tauri::generate_handler![
            term_attach,
            term_input,
            term_resize,
            term_ack,
            term_request_full
        ])
        .build(tauri::generate_context!())
        .expect("failed to build the app")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // Dropping the session hangs up on the shell.
                if let Some(state) = app.try_state::<Term>() {
                    state.session.lock().unwrap().take();
                }
            }
        });
}
