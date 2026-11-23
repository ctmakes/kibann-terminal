//! The `kibann` command line on its own (the app's executable is it too).

fn main() {
    std::process::exit(cli::run(std::env::args_os().skip(1).collect()));
}
