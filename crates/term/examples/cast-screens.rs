//! Print a recording's title and screen just before each input, and at the
//! end: `cargo run -p term --example cast-screens -- FILE.cast [--all]`.
//! For writing agent-detection tests against real recordings.

use proto::Resize;
use proto::wire::ReadSource;
use term::Emulator;
use term::cast::{CastEvent, parse};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: cast-screens FILE.cast");
    let text = std::fs::read_to_string(&path).expect("read cast");
    let (header, events) = parse(&text).expect("parse cast");
    let mut emu = Emulator::new(Resize {
        cols: header.width,
        rows: header.height,
        cell_w: 8,
        cell_h: 16,
    })
    .expect("emulator");
    let show = |emu: &Emulator, label: &str| {
        let screen = term::read::text(emu.terminal(), emu.size(), ReadSource::Visible, None)
            .unwrap_or_default();
        println!("===== {label} | title {:?}", emu.title());
        println!("{screen}");
    };
    for e in &events {
        match e {
            CastEvent::Output(_, data) => emu.write(data.as_bytes()),
            CastEvent::Input(t, data) => show(&emu, &format!("before input at {t:.2}s: {data:?}")),
            CastEvent::Resize(..) => {}
        }
    }
    show(&emu, "end");
}
