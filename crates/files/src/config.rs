//! The config file: `~/.config/kibann/config` (or `$XDG_CONFIG_HOME/kibann`,
//! or `$KIBANN_CONFIG_DIR`), Ghostty syntax. Read and written the same way
//! by the app and the CLI: edits keep the other lines and replace the file
//! whole, so a reader never sees half of one.

use std::path::PathBuf;

use proto::theme::{ThemeChoice, config_theme, set_config_theme};

/// Where the config file and the user's themes live.
pub fn config_dir() -> Option<PathBuf> {
    proto::paths::config_dir()
}

pub fn config_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config"))
}

fn text() -> String {
    config_file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .unwrap_or_default()
}

/// The config file's theme choice (the defaults without a file or a
/// `theme` line), and its problems.
pub fn read_choice() -> (ThemeChoice, Vec<String>) {
    let (choice, errors) = config_theme(&text());
    (choice.unwrap_or_default(), errors)
}

/// The config file's other settings (fonts, notifications), and their
/// problems.
pub fn read_settings() -> (proto::config::Settings, Vec<String>) {
    proto::config::Settings::parse(&text())
}

/// Save `choice` to the config file, keeping its other lines.
pub fn save_choice(choice: &ThemeChoice) -> Result<(), String> {
    edit(|old| set_config_theme(old, choice))
}

/// Set `key` to `values` in the config file (Settings), keeping its other
/// lines. Only the keys Settings shows, with values the parser takes.
pub fn save_values(key: &str, values: &[String]) -> Result<(), String> {
    proto::config::check_values(key, values)?;
    edit(|old| proto::config::set_config_values(old, key, values))
}

/// Rewrite the config file with `change`, made if missing.
fn edit(change: impl FnOnce(&str) -> String) -> Result<(), String> {
    let file = config_file().ok_or("no home directory")?;
    let old = std::fs::read_to_string(&file).unwrap_or_default();
    crate::write_whole(&file, &change(&old)).map_err(|e| e.to_string())
}
