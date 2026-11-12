//! kibannd: keeps the terminal sessions running while the app is closed.
//!
//! Started by the app; not meant to be run by hand, though it can be:
//! `kibannd [--socket PATH] [--idle-exit SECONDS]`.

#[cfg(unix)]
fn main() {
    std::process::exit(daemon::run_daemon(std::env::args_os().skip(1)));
}

#[cfg(not(unix))]
fn main() {
    eprintln!("kibannd needs Unix sockets; on this platform the app runs sessions itself");
    std::process::exit(1);
}
