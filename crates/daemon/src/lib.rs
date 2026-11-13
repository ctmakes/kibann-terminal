//! The daemon owns the terminal sessions so they outlive the app, and the
//! client is how the app (and later the CLI) talks to it.
//!
//! The server core ([`server::Daemon`]) serves any byte stream. On Unix it
//! listens on a socket ([`server::serve_socket`]) and runs as `kibannd`; where
//! there is no socket support yet (Windows), the app runs the same core in its
//! own process over in-memory pipes ([`client::Client::in_process`]).

pub mod cli;
pub mod client;
pub mod layout;
pub mod outbox;
pub mod paths;
pub mod pipe;
pub mod saved;
pub mod server;
pub mod status;
pub mod view;

/// Identifies a daemon's executable: its size and modification time. The
/// app compares it with the executable it would start, so a rebuilt or
/// upgraded binary is noticed.
pub fn build_id(path: &std::path::Path) -> std::io::Result<String> {
    let meta = std::fs::metadata(path)?;
    let modified = meta
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    Ok(format!("{}-{}", meta.len(), modified.as_nanos()))
}

/// What shells learn about this daemon: its socket, and a `kibann` link to
/// this executable on `PATH` (in `<socket>.bin/` beside the socket, so a dev
/// and an installed copy each find their own).
#[cfg(unix)]
fn pane_env(socket: &std::path::Path) -> server::PaneEnv {
    let bin = std::env::current_exe().ok();
    let path_dir = bin.as_ref().and_then(|exe| {
        let dir = socket.with_extension("bin");
        std::fs::create_dir_all(&dir).ok()?;
        let link = dir.join("kibann");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(exe, &link).ok()?;
        Some(dir)
    });
    server::PaneEnv {
        socket: Some(socket.to_owned()),
        bin,
        path_dir,
    }
}

/// `main` for the executables that hold the daemon and the CLI (`kibannd`;
/// the app has its own and calls the same pieces): `pane ...`, `tab ...` and
/// `skill ...` are CLI commands, anything else runs the daemon.
#[cfg(unix)]
pub fn main(args: Vec<std::ffi::OsString>) -> i32 {
    if cli::is_command(args.first()) {
        cli::run(args)
    } else {
        run_daemon(args.into_iter())
    }
}

/// The file to keep the layout in, if any: not for a daemon on a socket of
/// its own (tests, a private daemon) unless `KIBANN_STATE_DIR` says where.
pub fn layout_file(own_socket: bool) -> Option<std::path::PathBuf> {
    if own_socket && std::env::var_os("KIBANN_STATE_DIR").is_none() {
        return None;
    }
    paths::layout_file(cfg!(debug_assertions))
}

/// The daemon's `main`, shared by `kibannd` and the app's `--daemon` mode
/// (used by dev builds). `args` are the arguments after the program name.
/// Returns the exit code.
#[cfg(unix)]
pub fn run_daemon(args: impl Iterator<Item = std::ffi::OsString>) -> i32 {
    use std::time::Duration;

    let mut socket = None;
    let mut idle = 10u64;
    let mut args = args;
    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--socket") => socket = args.next().map(std::path::PathBuf::from),
            Some("--idle-exit") => {
                idle = args
                    .next()
                    .and_then(|v| v.to_str()?.parse().ok())
                    .unwrap_or(idle);
            }
            _ => {
                eprintln!("usage: kibannd [--socket PATH] [--idle-exit SECONDS]");
                return 2;
            }
        }
    }
    let own_socket = socket.is_some() || std::env::var_os("KIBANN_SOCKET").is_some();
    let socket = socket.unwrap_or_else(|| paths::socket_path(cfg!(debug_assertions)));
    // Read at start: the file may be rebuilt while we run.
    let build = std::env::current_exe()
        .and_then(|p| build_id(&p))
        .unwrap_or_else(|_| "unknown".into());
    let d = server::Daemon::new(build);
    d.set_pane_env(pane_env(&socket));
    if let Some(f) = layout_file(own_socket) {
        d.save_layout_to(f, saved::SAVE_DELAY);
    }
    let cfg = server::SocketConfig {
        socket,
        idle_exit: Duration::from_secs(idle),
    };
    let pid = std::process::id();
    eprintln!("kibannd {pid} listening on {}", cfg.socket.display());
    match server::serve_socket(&d, &cfg) {
        Ok(()) => {
            eprintln!("kibannd {pid} exiting");
            0
        }
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            // Another daemon won the race to start; clients use that one.
            eprintln!("kibannd {pid}: {e}");
            0
        }
        Err(e) => {
            eprintln!("kibannd {pid}: {e}");
            1
        }
    }
}
