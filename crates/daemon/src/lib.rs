//! The daemon owns the terminal sessions so they outlive the app. It's the
//! one long-lived process, so it holds only what has to live with the
//! sessions (them, the layout, agent status, git context); restarting it
//! ends every shell. Programs talk to it with the `client` crate.
//!
//! The server core ([`server::Daemon`]) serves any byte stream. On Unix it
//! listens on a socket ([`server::serve_socket`]); tests also run it in
//! their own process over in-memory pipes ([`in_process`]).

pub mod installed;
pub mod layout;
pub mod outbox;
pub mod pipe;
pub mod saved;
pub mod server;
pub mod status;

use proto::paths;

/// Run the daemon inside this process, over in-memory pipes (for tests).
/// Its sessions end with the client.
pub fn in_process(
    on_event: impl FnMut(client::ClientEvent<'_>) + Send + 'static,
) -> std::io::Result<client::Client> {
    let daemon = server::Daemon::new("in-process");
    if let Some(f) = layout_file(std::env::var_os("KIBANN_SOCKET").is_some()) {
        daemon.save_layout_to(f, saved::SAVE_DELAY);
    }
    let (to_daemon, daemon_reads) = pipe::pipe();
    let (daemon_writes, from_daemon) = pipe::pipe();
    daemon.serve(daemon_reads, daemon_writes)?;
    Ok(
        client::Client::new(from_daemon, to_daemon, on_event)?.on_drop(move || {
            let _ = &daemon;
        }),
    )
}

/// Close every descriptor but stdin, stdout and stderr. The daemon
/// outlives whoever started it, so it mustn't keep their files open: on
/// macOS a `fork` in another of the starter's threads can hand the child
/// descriptors that weren't close-on-exec yet, like the pipe on which that
/// thread's own `spawn` waits for its child's `exec` (which then never
/// returns while the daemon lives).
#[cfg(unix)]
fn close_inherited() {
    let Ok(dir) = std::fs::read_dir("/dev/fd") else {
        return;
    };
    // Listed first, closed after: reading the directory uses a descriptor.
    let fds: Vec<i32> = dir
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok())
        .filter(|fd| *fd > 2)
        .collect();
    for fd in fds {
        // SAFETY: nothing in this process uses these yet; the one that was
        // reading /dev/fd is already closed (EBADF, ignored).
        unsafe { libc::close(fd) };
    }
}

/// Identifies the daemon's code: a fingerprint of the source it's built
/// from (see `build.rs`). The app compares a running daemon's with its own,
/// so an update that changes how the terminal behaves is noticed, and one
/// that doesn't keeps the daemon and its shells.
pub const BUILD: &str = env!("KIBANN_DAEMON_BUILD");

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
        // Split on whitespace.
        command: std::env::var("KIBANN_COMMAND")
            .ok()
            .map(|c| c.split_whitespace().map(String::from).collect::<Vec<_>>())
            .filter(|c| !c.is_empty()),
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

/// The daemon's `main`, for `kibannd` (the tests' daemon) and the app's
/// `--daemon` mode (what the app starts). `args` are the arguments after
/// `--daemon`.
/// Returns the exit code.
#[cfg(unix)]
pub fn run_daemon(args: impl Iterator<Item = std::ffi::OsString>) -> i32 {
    use std::time::Duration;

    #[cfg(unix)]
    close_inherited();

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
    let dev = cfg!(debug_assertions);
    let socket = socket.unwrap_or_else(|| paths::socket_path(dev));
    // The app always passes `--socket`, so only a socket other than the
    // default one counts as a test's own (which keeps no layout).
    let own_socket = socket != paths::default_socket(dev);
    let d = server::Daemon::new(BUILD);
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
