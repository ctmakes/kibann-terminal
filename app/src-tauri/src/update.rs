//! Updates from GitHub releases: checked at start and every few hours,
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
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// The first look, once the window is up and shells have started.
const FIRST: Duration = Duration::from_secs(20);

/// A downloaded update, waiting for the user to restart.
#[derive(Default)]
pub struct Ready(Mutex<Option<(Update, Vec<u8>)>>);

#[derive(Clone, Serialize)]
struct UpdateReady {
    version: String,
    notes: String,
}

/// Look for updates now and then. Not in dev builds.
pub fn start(app: &AppHandle) {
    app.manage(Ready::default());
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
    let found = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let update = match found {
        Ok(Some(u)) => u,
        Ok(None) => {
            let v = app.package_info().version.to_string();
            say(format!("Kibann Terminal {v} is the newest version."));
            return;
        }
        Err(e) => {
            eprintln!("kibann: update check: {e}");
            say(format!("Couldn't check for updates: {e}"));
            return;
        }
    };
    match update.download(|_, _| {}, || {}).await {
        Ok(bytes) => {
            ready(app, &update);
            *app.state::<Ready>().0.lock().unwrap() = Some((update, bytes));
        }
        Err(e) => {
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

/// Kibann Terminal → Check for Updates….
#[tauri::command]
pub async fn update_check(app: AppHandle) {
    check(&app, true).await;
}
