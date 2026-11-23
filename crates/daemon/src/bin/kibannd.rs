//! kibannd: the daemon on its own, for tests. The app runs the same code as
//! `kibann --daemon`: `kibannd [--socket PATH] [--idle-exit SECONDS]`.

fn main() {
    std::process::exit(daemon::run_daemon(std::env::args_os().skip(1)));
}
