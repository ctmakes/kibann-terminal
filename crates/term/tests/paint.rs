//! The paint fixtures' synthetic screens (`fixtures/paint/inputs/`: `box`,
//! `styles`, `wide`), drawn by the emulator from escape sequences, so their
//! cells, widths and styles are what a program's output really gives.
//!
//! `UPDATE_PAINT=1 cargo test -p term --test paint` rewrites them; `npm run
//! gen:paint` in `app/` does that, then rewrites the painter's answers.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use proto::Resize;
use proto::theme::Theme;
use term::emulator::Emulator;

const FG: u32 = 0xd8d8d8;
const BG: u32 = 0x181818;
const PALETTE: [u32; 16] = [
    0x000000, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5, 0x666666,
    0xf14c4c, 0x23d18b, 0xf5f543, 0x3b8eea, 0xd670d6, 0x29b8db, 0xffffff,
];

/// Escape sequences, built up.
#[derive(Default)]
struct Vt(String);

impl Vt {
    fn at(&mut self, x: u16, y: u16) -> &mut Self {
        let _ = write!(self.0, "\x1b[{};{}H", y + 1, x + 1);
        self
    }

    /// SGR parameters (`1`, `38;2;r;g;b`, `4:3`…); `0` resets.
    fn sgr(&mut self, params: &str) -> &mut Self {
        let _ = write!(self.0, "\x1b[{params}m");
        self
    }

    fn text(&mut self, s: &str) -> &mut Self {
        self.0.push_str(s);
        self
    }
}

fn fg(c: u32) -> String {
    format!("38;2;{};{};{}", c >> 16, (c >> 8) & 0xff, c & 0xff)
}

fn bg(c: u32) -> String {
    format!("48;2;{};{};{}", c >> 16, (c >> 8) & 0xff, c & 0xff)
}

/// The underline's color.
fn ul(c: u32) -> String {
    format!("58;2;{};{};{}", c >> 16, (c >> 8) & 0xff, c & 0xff)
}

/// `vt` on a `cols`×`rows` screen with the cursor hidden: one full frame.
fn screen(cols: u16, rows: u16, vt: &Vt) -> String {
    let mut emu = Emulator::new(Resize {
        cols,
        rows,
        cell_w: 8,
        cell_h: 16,
    })
    .unwrap();
    emu.set_theme(&Theme {
        palette: PALETTE.to_vec(),
        background: BG,
        foreground: FG,
        cursor: None,
        cursor_text: None,
        selection_bg: None,
        selection_fg: None,
    })
    .unwrap();
    emu.write(vt.0.as_bytes());
    emu.write(b"\x1b[?25l");
    let mut frame = emu.build_frame(true).unwrap().expect("a frame");
    frame.seq = 1;
    serde_json::to_string(&frame).unwrap() + "\n"
}

/// Every box-drawing, block, braille and Powerline character, 64 a row; odd
/// rows on a background (double lines show it in their gaps).
fn box_screen() -> String {
    let cps: Vec<u32> = (0x2500..=0x259f)
        .chain(0x2800..=0x28ff)
        .chain(0xe0b0..=0xe0b3)
        .collect();
    let cols = 64u16;
    let mut vt = Vt::default();
    for (y, line) in cps.chunks(cols.into()).enumerate() {
        let y = y as u16;
        vt.at(0, y);
        if y % 2 == 1 {
            vt.sgr(&format!("{};{}", fg(0x0dbc79), bg(0x202040)));
        } else {
            vt.sgr(&fg(FG));
        }
        let s: String = line.iter().filter_map(|&c| char::from_u32(c)).collect();
        vt.text(&s).sgr("0");
    }
    screen(cols, cps.len().div_ceil(cols.into()) as u16, &vt)
}

/// Colors, fonts, faint, invisible and every decoration.
fn styles_screen() -> String {
    let mut vt = Vt::default();
    for (i, &c) in PALETTE.iter().enumerate() {
        let x = i as u16 * 3;
        vt.at(x, 0).sgr(&fg(c)).text("Ab").sgr("0");
        vt.at(x, 1)
            .sgr(&format!("{};{}", fg(FG), bg(c)))
            .text("Ab")
            .sgr("0");
    }
    vt.at(0, 2).text("regular");
    vt.at(8, 2).sgr("1").text("bold").sgr("0");
    vt.at(13, 2).sgr("3").text("italic").sgr("0");
    vt.at(20, 2).sgr("1;3").text("both").sgr("0");
    vt.at(25, 2).sgr("2").text("faint").sgr("0");
    vt.at(31, 2)
        .sgr(&format!("8;9;{}", bg(0x333333)))
        .text("hidden")
        .sgr("0");
    for u in 1..=5u16 {
        let y = 2 + u;
        vt.at(0, y)
            .sgr(&format!("4:{u}"))
            .text(&format!("underline {u}"))
            .sgr("0");
        vt.at(16, y)
            .sgr(&format!("4:{u};{}", ul(0xf14c4c)))
            .text(&format!("colored {u}"))
            .sgr("0");
        vt.at(32, y)
            .sgr(&format!("2;4:{u};{};{}", bg(0x202040), ul(0x3b8eea)))
            .text(&format!("faint {u}"))
            .sgr("0");
    }
    vt.at(0, 8).sgr("9").text("strike").sgr("0");
    vt.at(8, 8).sgr("53").text("overline").sgr("0");
    vt.at(18, 8)
        .sgr(&format!("9;53;4:2;{};{}", fg(0xe5e510), ul(0x23d18b)))
        .text("all")
        .sgr("0");
    // A decorated run that starts mid-cell-pattern and ends at the edge.
    vt.at(45, 9).sgr("4:3").text("end").sgr("0");
    vt.at(0, 10).text("mixed ");
    vt.sgr(&format!("1;{};{}", fg(0xcd3131), bg(0x101010)))
        .text("runs")
        .sgr("0");
    vt.sgr("4:5").text(" in a row").sgr("0");
    screen(48, 12, &vt)
}

/// Wide and composed graphemes, including one that doesn't fit at the edge.
fn wide_screen() -> String {
    let mut vt = Vt::default();
    // Mode 2027: graphemes as one cell (ZWJ sequences, flags), as Ghostty
    // and the programs that ask for it draw them.
    vt.text("\x1b[?2027h");
    vt.at(0, 0).text("日本語のテキスト");
    vt.at(0, 1).text("a😀b👩‍💻c🇯🇵d");
    // Combining marks: a letter and its accent in one cell.
    vt.at(0, 2)
        .sgr(&fg(0xe5e510))
        .text("e\u{301} n\u{303} a\u{308}")
        .sgr("0");
    vt.at(0, 3).sgr(&bg(0x202040)).text("x日y本z").sgr("0");
    // Column 19 is the last: a wide character there wraps, leaving a
    // blank spacer at the edge. Autowrap off keeps it on the row.
    vt.at(19, 4).text("\x1b[?7l日\x1b[?7h");
    vt.at(0, 5).sgr("1").text("wide bold 太字").sgr("0");
    screen(20, 6, &vt)
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/paint/inputs")
}

#[test]
fn paint_inputs_match_the_emulator() {
    let update = std::env::var_os("UPDATE_PAINT").is_some();
    let mut stale = Vec::new();
    for (name, text) in [
        ("box", box_screen()),
        ("styles", styles_screen()),
        ("wide", wide_screen()),
    ] {
        let path = root().join(format!("{name}.jsonl"));
        if update {
            std::fs::create_dir_all(root()).unwrap();
            std::fs::write(&path, text).unwrap();
        } else if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            stale.push(name);
        }
    }
    assert!(
        stale.is_empty(),
        "stale paint inputs {stale:?}: run `npm run gen:paint` in app/"
    );
}
