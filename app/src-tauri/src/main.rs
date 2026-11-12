// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Dev builds run the daemon from this same executable; see `connect`.
    #[cfg(unix)]
    if std::env::args_os().nth(1).is_some_and(|a| a == "--daemon") {
        std::process::exit(daemon::run_daemon(std::env::args_os().skip(2)));
    }
    // `kibann pane split` and the like, from a shell in a pane.
    let first = std::env::args_os().nth(1);
    if daemon::cli::is_command(first.as_ref()) {
        std::process::exit(daemon::cli::run(std::env::args_os().skip(1).collect()));
    }
    kibann_lib::run()
}
