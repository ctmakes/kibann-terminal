//! Where the daemon's socket, lock and log live: a directory only the user
//! can open.

use std::path::{Path, PathBuf};

/// The socket file name. Dev builds use their own, so `tauri dev` never
/// touches the sessions of an installed copy.
pub fn socket_name(dev: bool) -> &'static str {
    if dev {
        "daemon-dev.sock"
    } else {
        "daemon.sock"
    }
}

/// The directory for the socket: `$TMPDIR/kibann` on macOS (per-user there),
/// `$XDG_RUNTIME_DIR/kibann` on Linux, else `/tmp/kibann-<uid>`.
pub fn runtime_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    if let Some(t) = std::env::var_os("TMPDIR") {
        return PathBuf::from(t).join("kibann");
    }
    #[cfg(unix)]
    {
        if let Some(t) = std::env::var_os("XDG_RUNTIME_DIR") {
            return PathBuf::from(t).join("kibann");
        }
        // SAFETY: geteuid has no preconditions.
        let uid = unsafe { libc::geteuid() };
        PathBuf::from(format!("/tmp/kibann-{uid}"))
    }
    #[cfg(not(unix))]
    std::env::temp_dir().join("kibann")
}

/// The socket a build uses when `KIBANN_SOCKET` doesn't name another.
pub fn default_socket(dev: bool) -> PathBuf {
    runtime_dir().join(socket_name(dev))
}

/// `KIBANN_SOCKET` if set, else the default socket for this kind of build.
pub fn socket_path(dev: bool) -> PathBuf {
    match std::env::var_os("KIBANN_SOCKET") {
        Some(p) => PathBuf::from(p),
        None => default_socket(dev),
    }
}

/// Where the layout is kept between sessions (the daemon's `saved` module):
/// `KIBANN_STATE_DIR` if set, else `~/Library/Application Support/kibann`
/// on macOS, `$XDG_STATE_HOME/kibann` (`~/.local/state/kibann`) on Linux,
/// `%APPDATA%\kibann` on Windows. Dev builds keep their own file.
pub fn layout_file(dev: bool) -> Option<PathBuf> {
    let name = if dev {
        "layout-dev.json"
    } else {
        "layout.json"
    };
    if let Some(d) = std::env::var_os("KIBANN_STATE_DIR") {
        return Some(PathBuf::from(d).join(name));
    }
    #[cfg(target_os = "macos")]
    let dir = home_dir().map(|h| h.join("Library/Application Support/kibann"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let dir = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".local/state")))
        .map(|d| d.join("kibann"));
    #[cfg(windows)]
    let dir = std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("kibann"));
    dir.map(|d| d.join(name))
}

/// The user's home directory.
///
/// `HOME` on Unix. On Windows `USERPROFILE` first: a `HOME` there is often
/// a Unix-style one (`/c/Users/me`) that Git Bash or MSYS set.
pub fn home_dir() -> Option<PathBuf> {
    let vars: &[&str] = if cfg!(windows) {
        &["USERPROFILE", "HOME"]
    } else {
        &["HOME"]
    };
    vars.iter()
        .find_map(|v| std::env::var_os(v).filter(|h| !h.is_empty()))
        .map(PathBuf::from)
}

/// What follows `~/` in `raw` (also `~\\` on Windows): a path under the
/// home directory.
pub fn under_home(raw: &str) -> Option<&str> {
    raw.strip_prefix("~/")
        .or_else(|| raw.strip_prefix("~\\").filter(|_| cfg!(windows)))
}

/// Where `program` is on `PATH`, if it's there. On Windows a name without
/// an extension also matches with each one in `PATHEXT` (`.EXE`, `.CMD`…).
pub fn on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) && Path::new(program).extension().is_none() {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(str::to_owned)
            .collect()
    } else {
        vec![String::new()]
    };
    std::env::split_paths(&path).find_map(|dir| {
        exts.iter()
            .map(|e| dir.join(format!("{program}{e}")))
            .find(|p| p.is_file())
    })
}

/// Where the config file, the user's themes, workspaces, screens and parts live:
/// `KIBANN_CONFIG_DIR` if set, else `$XDG_CONFIG_HOME/kibann`, else
/// `~/.config/kibann`.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("KIBANN_CONFIG_DIR") {
        return Some(PathBuf::from(d));
    }
    if let Some(d) = std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d).join("kibann"));
    }
    Some(home_dir()?.join(".config").join("kibann"))
}

/// The user's screens: `<config>/screens/NAME/screen.json`.
pub fn screens_dir() -> Option<PathBuf> {
    config_dir().map(|d| d.join("screens"))
}

/// The user's parts: `<config>/parts/NAME/part.json` and its page.
pub fn parts_dir() -> Option<PathBuf> {
    config_dir().map(|d| d.join("parts"))
}

/// The workspaces the user keeps: `<config>/workspaces/NAME.toml`.
pub fn workspaces_dir() -> Option<PathBuf> {
    config_dir().map(|d| d.join("workspaces"))
}

/// The files (workspaces, parts) the user said may run their commands (see
/// the `files` crate), next to the saved layout.
pub fn trusted_file() -> Option<PathBuf> {
    layout_file(false)?.parent().map(|d| d.join("trusted.json"))
}

/// Make the socket's directory, readable by the user only, and check that
/// nobody else owns it (another user could otherwise pre-create it).
#[cfg(unix)]
pub fn prepare_dir(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let meta = std::fs::metadata(dir)?;
    // SAFETY: geteuid has no preconditions.
    if meta.uid() != unsafe { libc::geteuid() } {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("{} belongs to another user", dir.display()),
        ));
    }
    if meta.mode() & 0o077 != 0 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn the_directory_is_made_private() {
        let dir = std::env::temp_dir().join(format!("kibann-paths-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        prepare_dir(&dir).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
