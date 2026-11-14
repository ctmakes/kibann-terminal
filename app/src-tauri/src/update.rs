//! Updates from GitHub releases: checked at start and every hour,
//! downloaded in the background, installed when the user says (a notice
//! with a Restart button, or Kibann Terminal → Check for Updates…).
//!
//! Restarting keeps the shells: the daemon carries on, and the new app uses
//! it when it speaks the same protocol (see `decide` in lib.rs).

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

/// How often to look for a new version while the app runs.
const EVERY: Duration = Duration::from_secs(60 * 60);
/// The first look, once the window is up and shells have started.
const FIRST: Duration = Duration::from_secs(20);

/// A downloaded update, waiting for the user to restart.
#[derive(Default)]
pub struct Ready(Mutex<Option<(Update, Vec<u8>)>>);

/// Where updating stands, for Settings → Updates.
#[derive(Clone, Serialize)]
pub struct Status {
    /// "idle" (not checked yet), "checking", "downloading", "ready",
    /// "latest" (none newer), "error", or "dev" (dev builds don't update).
    state: &'static str,
    /// The version found (downloading or ready).
    version: Option<String>,
    error: Option<String>,
    /// When the last check ended, in ms since 1970.
    checked: Option<u64>,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            state: if tauri::is_dev() { "dev" } else { "idle" },
            version: None,
            error: None,
            checked: None,
        }
    }
}

#[derive(Default)]
pub struct Now(Mutex<Status>);

/// Change the status and tell the window.
fn set(app: &AppHandle, change: impl FnOnce(&mut Status)) {
    let s = {
        let now = app.state::<Now>();
        let mut s = now.0.lock().unwrap();
        change(&mut s);
        s.clone()
    };
    let _ = app.emit("update-status", s);
}

fn now_ms() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
}

#[derive(Clone, Serialize)]
struct UpdateReady {
    version: String,
    notes: String,
}

/// Look for updates now and then. Not in dev builds.
pub fn start(app: &AppHandle) {
    app.manage(Ready::default());
    app.manage(Now::default());
    if tauri::is_dev() {
        return;
    }
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("updates".into())
        .spawn(move || {
            std::thread::sleep(FIRST);
            loop {
                tauri::async_runtime::block_on(check(&app, false));
                std::thread::sleep(EVERY);
            }
        });
}

/// Look for a new version; download it and say it's ready. `asked`: the
/// user chose Check for Updates, so say what was found either way.
pub async fn check(app: &AppHandle, asked: bool) {
    let say = |text: String| {
        if asked {
            let _ = app.emit("notice", text);
        }
    };
    if tauri::is_dev() {
        say("Updates aren't checked in dev builds.".into());
        return;
    }
    // A version found earlier and downloaded: say so again.
    if let Some((u, _)) = app.state::<Ready>().0.lock().unwrap().as_ref() {
        if asked {
            ready(app, u);
        }
        return;
    }
    set(app, |s| {
        s.state = "checking";
        s.error = None;
    });
    let found = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let update = match found {
        Ok(Some(u)) => u,
        Ok(None) => {
            set(app, |s| {
                s.state = "latest";
                s.checked = now_ms();
            });
            let v = app.package_info().version.to_string();
            say(format!("Kibann Terminal {v} is the newest version."));
            return;
        }
        Err(e) => {
            set(app, |s| {
                s.state = "error";
                s.error = Some(e.to_string());
                s.checked = now_ms();
            });
            eprintln!("kibann: update check: {e}");
            say(format!("Couldn't check for updates: {e}"));
            return;
        }
    };
    set(app, |s| {
        s.state = "downloading";
        s.version = Some(update.version.clone());
        s.checked = now_ms();
    });
    match update.download(|_, _| {}, || {}).await {
        Ok(bytes) => {
            set(app, |s| s.state = "ready");
            ready(app, &update);
            #[cfg(target_os = "macos")]
            crate::tray::set_update(app, update.version.clone());
            *app.state::<Ready>().0.lock().unwrap() = Some((update, bytes));
        }
        Err(e) => {
            set(app, |s| {
                s.state = "error";
                s.error = Some(format!("couldn't download {}: {e}", update.version));
            });
            eprintln!("kibann: update download: {e}");
            say(format!("Couldn't download version {}: {e}", update.version));
        }
    }
}

fn ready(app: &AppHandle, u: &Update) {
    let _ = app.emit(
        "update-ready",
        UpdateReady {
            version: u.version.clone(),
            notes: u.body.clone().unwrap_or_default(),
        },
    );
}

/// Install the downloaded update and start the new version.
#[tauri::command]
pub fn update_install(app: AppHandle) -> Result<(), String> {
    let Some((update, bytes)) = app.state::<Ready>().0.lock().unwrap().take() else {
        return Err("no update is ready".into());
    };
    update.install(bytes).map_err(|e| e.to_string())?;
    app.restart();
}

/// Kibann Terminal → Check for Updates…, or Settings → Updates (`quiet`:
/// the answer shows there, not as a notice).
#[tauri::command]
pub async fn update_check(app: AppHandle, quiet: Option<bool>) {
    check(&app, !quiet.unwrap_or(false)).await;
}

/// Where updating stands.
#[tauri::command]
pub fn update_status(now: tauri::State<'_, Now>) -> Status {
    now.0.lock().unwrap().clone()
}
