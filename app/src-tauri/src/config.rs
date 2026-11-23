//! The config file: read again when it changes, its problems, and the
//! commands Settings uses.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::event;
use crate::looks::{Looks, show_theme};
#[cfg(target_os = "macos")]
use crate::menu::set_quit_labels;
use crate::screens;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use crate::{fonts, tray};

/// Read the config file again (it changed, or at start).
pub(crate) fn reload_config(app: &AppHandle) {
    let (settings, problems) = files::config::read_settings();
    for e in &problems {
        let _ = app.emit(event::NOTICE, format!("Config: {e}"));
    }
    *app.state::<Problems>().0.lock().unwrap() = problems;
    app.state::<Mutex<Looks>>().lock().unwrap().choice = settings.theme.clone();
    // Settings, if open, shows the file's problems and themes.
    let _ = app.emit(event::CONFIG_INFO, config_info(app.clone()));
    show_theme(app);
    let changed = {
        let state = app.state::<Mutex<proto::config::Settings>>();
        let mut now = state.lock().unwrap();
        let changed = *now != settings;
        *now = settings.clone();
        changed
    };
    if changed {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        tray::set_visible(app, settings.menu_bar_icon);
        #[cfg(target_os = "macos")]
        set_quit_labels(app, settings.menu_bar_icon);
        let _ = app.emit(event::SETTINGS, settings);
    }
}

/// The config file's lines that couldn't be used, as of its last read.
#[derive(Default)]
pub(crate) struct Problems(pub(crate) Mutex<Vec<String>>);

/// What Settings shows about the config file.
#[derive(Clone, Serialize, ts_rs::TS)]
#[ts(export)]
pub(crate) struct ConfigInfo {
    pub(crate) path: String,
    pub(crate) problems: Vec<String>,
    /// The themes chosen for each appearance.
    pub(crate) light: String,
    pub(crate) dark: String,
}

#[tauri::command]
pub fn config_info(app: AppHandle) -> ConfigInfo {
    let choice = app.state::<Mutex<Looks>>().lock().unwrap().choice.clone();
    ConfigInfo {
        path: files::config::config_file()
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
pub fn settings_set(key: String, values: Vec<String>) -> Result<(), String> {
    files::config::save_values(&key, &values)
}

/// Open the config file in the text editor, or show it in the file
/// manager (`reveal`); made first if it doesn't exist.
#[tauri::command]
pub fn config_open(app: AppHandle, reveal: bool) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let file = files::config::config_file().ok_or("no home directory")?;
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
    // Windows asks which app opens a file without an extension.
    #[cfg(windows)]
    return std::process::Command::new("notepad.exe")
        .arg(&file)
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string());
    #[cfg(not(any(target_os = "macos", windows)))]
    app.opener()
        .open_path(file.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// The installed monospaced fonts, or `None` where they can't be listed
/// (Settings then takes a typed name).
#[tauri::command]
pub async fn fonts_list() -> Option<Vec<String>> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    return Some(fonts::monospaced()).filter(|f| !f.is_empty());
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    None
}

#[tauri::command]
pub fn settings_current(
    settings: State<'_, Mutex<proto::config::Settings>>,
) -> proto::config::Settings {
    settings.lock().unwrap().clone()
}

/// Poll the config file and the user's themes: editors save in ways
/// (rename over) that file watchers report unreliably, and once a second
/// costs nothing.
pub(crate) fn watch_config(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("config-watch".into())
        .spawn(move || {
            let mut last = files::themes::stamp();
            let mut screens_last = files::screens::stamp();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let now = files::themes::stamp();
                if now != last {
                    last = now;
                    reload_config(&app);
                }
                screens::poll(&app, &mut screens_last);
            }
        });
}
