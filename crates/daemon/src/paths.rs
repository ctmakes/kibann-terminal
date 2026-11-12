//! Where the daemon's socket, lock and log live: a directory only the user
//! can open.

use std::path::PathBuf;

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

/// `KIBANN_SOCKET` if set, else the default socket for this kind of build.
pub fn socket_path(dev: bool) -> PathBuf {
    match std::env::var_os("KIBANN_SOCKET") {
        Some(p) => PathBuf::from(p),
        None => runtime_dir().join(socket_name(dev)),
    }
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
