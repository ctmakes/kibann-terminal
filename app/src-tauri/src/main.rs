// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Dev builds run the daemon from this same executable; see `connect`.
    #[cfg(unix)]
    if std::env::args_os().nth(1).is_some_and(|a| a == "--daemon") {
        std::process::exit(daemon::run_daemon(std::env::args_os().skip(2)));
    }
    kibann_lib::run()
}
