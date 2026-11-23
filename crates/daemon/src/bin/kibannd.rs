//! kibannd: the daemon on its own, for tests. The app runs the same code as
//! `kibann --daemon`: `kibannd [--socket PATH] [--idle-exit SECONDS]`.

#[cfg(unix)]
fn main() {
    std::process::exit(daemon::run_daemon(std::env::args_os().skip(1)));
}

#[cfg(not(unix))]
fn main() {
    eprintln!("kibannd needs Unix sockets; on this platform the app runs sessions itself");
    std::process::exit(1);
}
