// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The daemon runs from this same executable (`kibann --daemon`).
    #[cfg(unix)]
    if std::env::args_os().nth(1).is_some_and(|a| a == "--daemon") {
        std::process::exit(daemon::run_daemon(std::env::args_os().skip(2)));
    }
    // `kibann pane split` and the like, from a shell in a pane.
    let first = std::env::args_os().nth(1);
    if cli::is_command(first.as_ref()) {
        std::process::exit(cli::run(std::env::args_os().skip(1).collect()));
    }
    // A dev build started from a shell in the installed app inherits that
    // app's socket in `KIBANN_SOCKET`; following it would replace the
    // installed app's daemon and end its sessions. A test's own socket is
    // still honoured.
    #[cfg(debug_assertions)]
    if std::env::var_os("KIBANN_SOCKET")
        .is_some_and(|s| std::path::Path::new(&s) == proto::paths::default_socket(false))
    {
        // SAFETY: no other threads have started yet.
        unsafe { std::env::remove_var("KIBANN_SOCKET") };
    }
    kibann_lib::run()
}
