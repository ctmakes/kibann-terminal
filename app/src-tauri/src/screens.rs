//! Screens for the window (see `daemon::screens`): the one the config
//! names, ready to show; serving parts' pages (`kpart://`) with a policy
//! that keeps them to what they were allowed; asking before a part gets
//! its permissions; and running programs for parts that may.

use std::collections::HashSet;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use daemon::screens;
use proto::screen::{ScreenEntry, ScreenNow};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// The scheme parts are served from.
pub const SCHEME: &str = "kpart";

/// Where parts' files are, as the page sees them.
pub fn base() -> &'static str {
    if cfg!(windows) {
        "http://kpart.localhost/"
    } else {
        "kpart://localhost/"
    }
}

fn origin() -> &'static str {
    base().trim_end_matches('/')
}

/// What's remembered between reads.
#[derive(Default)]
pub struct Screens {
    /// The last screen that read without a mistake, shown while the
    /// chosen one has one.
    last_good: Mutex<Option<ScreenNow>>,
    /// Parts the user said no to (folder and permissions) this run, so an
    /// edit doesn't ask again.
    refused: Mutex<HashSet<String>>,
    /// The config's screen, as of the last time the window was told.
    shown: Mutex<String>,
}

fn configured(app: &AppHandle) -> String {
    app.state::<Mutex<proto::config::Settings>>()
        .lock()
        .unwrap()
        .screen
        .clone()
}

/// The screen `name`, ready to show: what its parts may do, asked of the
/// user for a part that wants more than before.
pub fn load(app: &AppHandle, name: &str) -> Result<ScreenNow, String> {
    let mut now = screens::now(name, base())?;
    for (part, p) in now.parts.iter_mut() {
        let (m, origin) = screens::part(part)?;
        let Some(lines) = screens::needs_asking(&origin, &m) else {
            continue;
        };
        let screens::Origin::User(dir) = &origin else {
            continue;
        };
        let key = format!(
            "{}\n{}",
            dir.display(),
            serde_json::to_string(&m.permissions).unwrap_or_default()
        );
        if app
            .state::<Screens>()
            .refused
            .lock()
            .unwrap()
            .contains(&key)
        {
            continue;
        }
        if ask(app, name, part, &lines) {
            screens::trust(dir, &m).map_err(|e| e.to_string())?;
            p.granted = m.permissions.clone();
        } else {
            app.state::<Screens>().refused.lock().unwrap().insert(key);
        }
    }
    Ok(now)
}

fn ask(app: &AppHandle, screen: &str, part: &str, lines: &[String]) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let list: Vec<String> = lines.iter().map(|l| format!("• {l}")).collect();
    app.dialog()
        .message(format!(
            "The screen {screen} shows the part {part}, which asks to:\n\n{}\n\nAllow it only if you trust where it came from. Without, it shows but can't do these.",
            list.join("\n")
        ))
        .title(format!("Allow the part {part}?"))
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Allow".into(),
            "Don't Allow".into(),
        ))
        .blocking_show()
}

/// The screen the config names. When it can't be shown, the last good one
/// (or the default) comes back with `problem` saying why.
pub fn current(app: &AppHandle) -> ScreenNow {
    let name = configured(app);
    *app.state::<Screens>().shown.lock().unwrap() = name.clone();
    match load(app, &name) {
        Ok(now) => {
            *app.state::<Screens>().last_good.lock().unwrap() = Some(now.clone());
            now
        }
        Err(e) => {
            let last = app.state::<Screens>().last_good.lock().unwrap().clone();
            let mut now = last
                .or_else(|| load(app, proto::config::DEFAULT_SCREEN).ok())
                .unwrap_or_else(|| {
                    // The built-in default always reads; this is for a
                    // broken build only.
                    screens::now(proto::config::DEFAULT_SCREEN, base()).expect("the default screen")
                });
            now.problem = e;
            now
        }
    }
}

/// Tell the window when the config names another screen, or a screen or
/// part changed on disk. Called once a second.
pub fn poll(app: &AppHandle, last: &mut Vec<(std::path::PathBuf, Option<std::time::SystemTime>)>) {
    let stamp = screens::stamp();
    let renamed = *app.state::<Screens>().shown.lock().unwrap() != configured(app);
    if stamp != *last || renamed {
        *last = stamp;
        let _ = app.emit("screen", current(app));
    }
}

#[tauri::command]
pub async fn screen_current(app: AppHandle) -> ScreenNow {
    current(&app)
}

#[tauri::command]
pub async fn screen_list() -> Vec<ScreenEntry> {
    screens::list()
}

/// A screen to try (the picker).
#[tauri::command]
pub async fn screen_load(app: AppHandle, name: String) -> Result<ScreenNow, String> {
    load(&app, &name)
}

/// Keep `name`: the config's `screen`.
#[tauri::command]
pub async fn screen_choose(app: AppHandle, name: String) -> Result<(), String> {
    screens::read(&name)?;
    let values = if name == proto::config::DEFAULT_SCREEN {
        Vec::new()
    } else {
        vec![name]
    };
    crate::theme::save_values("screen", &values)?;
    crate::reload_choice(&app);
    let _ = app.emit("screen", current(&app));
    Ok(())
}

#[derive(Serialize)]
pub struct ExecOut {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// The most output kept from a program, per stream.
const OUTPUT_MAX: usize = 4 << 20;

/// Run `program` for the part `part`, if its permissions list it and the
/// user allowed them: without a shell, with the user's PATH, up to
/// `timeout` ms (30 s; at most 2 minutes).
#[tauri::command]
pub async fn part_exec(
    part: String,
    program: String,
    args: Vec<String>,
    cwd: Option<String>,
    timeout: Option<u64>,
) -> Result<ExecOut, String> {
    let (m, origin) = screens::part(&part)?;
    if !screens::granted(&origin, &m).exec.contains(&program) {
        return Err(format!("the part {part} may not run {program}"));
    }
    let path =
        daemon::installed::locate(&program).ok_or_else(|| format!("{program} isn't installed"))?;
    let home = daemon::paths::home_dir();
    let dir = match cwd.as_deref() {
        Some("~") => home.clone(),
        Some(c) if c.starts_with("~/") => home.as_ref().map(|h| h.join(&c[2..])),
        Some(c) => Some(c.into()),
        None => home.clone(),
    };
    let mut cmd = Command::new(path);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(d) = dir.filter(|d| d.is_dir()) {
        cmd.current_dir(d);
    }
    if let Some(p) = daemon::installed::user_path() {
        cmd.env("PATH", p);
    }
    let mut child = cmd.spawn().map_err(|e| format!("{program}: {e}"))?;
    let read = |r: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(r) = r {
                let _ = r.take(OUTPUT_MAX as u64).read_to_end(&mut buf);
            }
            String::from_utf8_lossy(&buf).into_owned()
        })
    };
    let out = read(
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let err = read(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let limit = Duration::from_millis(timeout.unwrap_or(30_000).clamp(100, 120_000));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let stdout = out.join().unwrap_or_default();
    let mut stderr = err.join().unwrap_or_default();
    if status.is_none() {
        stderr.push_str(&format!(
            "\n{program} took longer than {} ms and was stopped",
            limit.as_millis()
        ));
    }
    Ok(ExecOut {
        code: status.and_then(|s| s.code()),
        stdout,
        stderr,
    })
}

/// A part's file for the `kpart` scheme: served with a policy that holds
/// it to its permissions (no network unless allowed, nothing from
/// elsewhere).
pub fn respond(request: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    use tauri::http::Response;
    let path = request.uri().path().to_owned();
    match screens::serve(&path) {
        Some((bytes, mime)) => {
            let granted = screens::granted_for_path(&path);
            Response::builder()
                .header("Content-Type", mime)
                .header("Content-Security-Policy", screens::csp(origin(), &granted))
                // The frame's origin is opaque (sandboxed): its scripts and
                // fonts are cross-origin loads.
                .header("Access-Control-Allow-Origin", "*")
                .header("Cache-Control", "no-store")
                .header("X-Content-Type-Options", "nosniff")
                .body(bytes.into_owned())
                .unwrap()
        }
        None => Response::builder()
            .status(404)
            .header("Content-Type", "text/plain")
            .body(b"not found".to_vec())
            .unwrap(),
    }
}
