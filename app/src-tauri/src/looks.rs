//! The theme in use: the config's choice for the system's appearance (or
//! one being previewed), given to the daemon and the page, and the window's
//! colors around it.

use std::sync::Mutex;

use client::Client;
use proto::wire::ClientMsg;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::event;
use crate::link::Term;

/// Which theme is in use, and why.
pub(crate) struct Looks {
    /// From the config file.
    pub(crate) choice: proto::theme::ThemeChoice,
    /// The system appearance.
    pub(crate) dark: bool,
    /// The appearance the window is held to (the theme's), or `None` when
    /// it follows the system.
    pub(crate) forced: Option<bool>,
    /// Shown by the picker, not kept yet.
    pub(crate) preview: Option<String>,
    /// In use now.
    pub(crate) name: String,
    pub(crate) theme: proto::theme::Theme,
}

/// The theme in use and its name (`theme_current`, the `theme` event).
#[derive(Clone, Serialize, ts_rs::TS)]
#[ts(export)]
pub(crate) struct ThemeNow {
    pub(crate) name: String,
    pub(crate) theme: proto::theme::Theme,
}

/// Show the right theme (the preview, else the config's for the
/// appearance): tell the daemon, which colors the terminals, and the
/// frontend, which colors the window. A theme that can't be loaded falls
/// back to the built-in one, with a notice.
pub(crate) fn show_theme(app: &AppHandle) {
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
    let (name, theme) = match files::themes::load(&name) {
        Ok((t, errors)) => {
            for e in errors {
                let _ = app.emit(event::NOTICE, format!("Theme \"{name}\": {e}"));
            }
            (name, t)
        }
        Err(e) => {
            let _ = app.emit(event::NOTICE, format!("{e}; using the built-in theme"));
            let dark = looks.lock().unwrap().dark;
            let n = if dark {
                proto::theme::DEFAULT_DARK
            } else {
                proto::theme::DEFAULT_LIGHT
            };
            (n.to_owned(), files::themes::load(n).expect("built in").0)
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
    if let Some(term) = app.try_state::<Term>()
        && let Ok(c) = term.client()
    {
        send_theme(app, &c);
    }
    let _ = app.emit(event::THEME, ThemeNow { name, theme });
}

/// Give the service the theme in use (its shells' colors).
pub(crate) fn send_theme(app: &AppHandle, c: &Client) {
    let theme = app.state::<Mutex<Looks>>().lock().unwrap().theme.clone();
    let _ = c.request(|req| ClientMsg::SetTheme { req, theme });
}

/// The window around the webview in the theme's colors: the title bar's
/// light or dark appearance, and on macOS (a transparent title bar, see
/// `tauri.conf.json`) the strip behind the title in the tab bar's color.
pub(crate) fn window_colors(app: &AppHandle, theme: &proto::theme::Theme, force: Option<bool>) {
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
    // The tab bar's color, so a resize shows no seam.
    let bar = proto::theme::chrome(theme, "bar").unwrap_or(theme.background);
    let [_, r, g, b] = bar.to_be_bytes();
    let _ = w.set_background_color(Some(tauri::window::Color(r, g, b, 255)));
}

#[tauri::command]
pub fn theme_current(looks: State<'_, Mutex<Looks>>) -> ThemeNow {
    let l = looks.lock().unwrap();
    ThemeNow {
        name: l.name.clone(),
        theme: l.theme.clone(),
    }
}

#[tauri::command]
pub async fn theme_list() -> Vec<files::themes::Entry> {
    files::themes::list()
}

/// Show `name` for now; `None` goes back to the kept theme.
#[tauri::command]
pub async fn theme_preview(app: AppHandle, name: Option<String>) {
    app.state::<Mutex<Looks>>().lock().unwrap().preview = name;
    show_theme(&app);
}

/// Keep `name` for the current appearance, in the config file.
#[tauri::command]
pub async fn theme_choose(app: AppHandle, name: String) -> Result<(), String> {
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
    let saved = files::config::save_choice(&choice);
    show_theme(&app);
    saved
}
