//! The user's files, shared by the app and the CLI: the config file
//! ([`config`]) and themes ([`themes`]), workspace files (the format in
//! [`workspace`], where they're found and whether they may run their
//! commands in [`workspaces`]), and the screens and parts the window is
//! made of ([`screens`]). The daemon doesn't read any of them; it's given
//! what they say.

use std::io;
use std::path::Path;

pub mod config;
pub mod screens;
pub mod themes;
pub mod workspace;
pub mod workspaces;

/// Write `text` to `path` (its directory made if missing) beside it and
/// rename it over, so a reader never sees half a file.
pub fn write_whole(path: &Path, text: &str) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("no directory for the file"))?;
    std::fs::create_dir_all(dir)?;
    let mut name = std::ffi::OsString::from(".");
    name.push(path.file_name().unwrap_or_default());
    name.push(".tmp");
    let tmp = dir.join(name);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}
