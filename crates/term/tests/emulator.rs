use proto::{CursorStyle, Frame, InputEvent, KeyAction, MouseAction, MouseTracking, Resize, flags};
use term::Emulator;

const SIZE: Resize = Resize {
    cols: 20,
    rows: 5,
    cell_w: 10,
    cell_h: 20,
};

fn emu() -> Emulator {
    Emulator::new(SIZE).unwrap()
}

/// Row `y` of a frame as text, with blanks as spaces and wide tails dropped.
fn row_text(f: &Frame, y: u16) -> String {
    let row = f.rows.iter().find(|r| r.y == y).expect("row in frame");
    let mut s = String::new();
    for run in &row.runs {
        for i in 0..run.len as usize {
            let wide_tail = i > 0
                && run
                    .wide
                    .as_ref()
                    .is_some_and(|w| w.contains(&(i as u16 - 1)));
            match &run.text {
                Some(t) if !t[i].is_empty() => s.push_str(&t[i]),
                _ if wide_tail => {}
                _ => s.push(' '),
            }
        }
    }
    s.trim_end().to_owned()
}

fn key(code: &str, text: Option<&str>, mods: u16) -> InputEvent {
    InputEvent::Key {
        action: KeyAction::Press,
        code: code.into(),
        key: text.unwrap_or("").into(),
        text: text.map(Into::into),
        mods,
        composing: false,
    }
}

fn encode(e: &mut Emulator, ev: InputEvent) -> Vec<u8> {
    e.take_outbox();
    e.input(&ev).unwrap();
    e.take_outbox()
}

#[test]
fn first_frame_is_full_and_covers_every_row() {
    let mut e = emu();
    let f = e.build_frame(false).unwrap().unwrap();
    assert!(f.full);
    assert_eq!(f.seq, 1);
    assert_eq!((f.cols, f.rows_total), (20, 5));
    assert_eq!(f.rows.len(), 5);
    for r in &f.rows {
        let covered: u16 = r.runs.iter().map(|r| r.len).sum();
        assert_eq!(covered, 20);
        assert!(
            r.runs.iter().all(|r| r.text.is_none()),
            "blank screen has no text"
        );
    }
    assert!(f.cursor.in_viewport);
    assert_eq!((f.cursor.x, f.cursor.y), (0, 0));
}

#[test]
fn clean_terminal_builds_nothing() {
    let mut e = emu();
    e.build_frame(false).unwrap();
    assert!(e.build_frame(false).unwrap().is_none());
}

#[test]
fn partial_frame_has_only_changed_rows() {
    let mut e = emu();
    e.build_frame(false).unwrap();
    e.write(b"\x1b[3;1Hhello");
    let f = e.build_frame(false).unwrap().unwrap();
    assert!(!f.full);
    assert_eq!(f.seq, 2);
    let ys: Vec<u16> = f.rows.iter().map(|r| r.y).collect();
    // Row 0 is included because the cursor left it.
    assert_eq!(ys, vec![0, 2]);
    assert_eq!(row_text(&f, 2), "hello");
    assert_eq!((f.cursor.x, f.cursor.y), (5, 2));
}

#[test]
fn styles_split_runs_and_resolve_colors() {
    let mut e = emu();
    // Red bold "ab", default "c", inverse "d", curly underline in blue "e".
    e.write(b"\x1b[1;31mab\x1b[0mc\x1b[7md\x1b[0m\x1b[4:3;58;2;0;0;255me\x1b[0m");
    let f = e.build_frame(false).unwrap().unwrap();
    let row = &f.rows[0];
    let st = |x: u16| {
        let r = row
            .runs
            .iter()
            .find(|r| x >= r.x && x < r.x + r.len)
            .unwrap();
        f.styles[r.s as usize]
    };
    let red = st(0);
    assert_eq!(red.flags & flags::BOLD, flags::BOLD);
    assert_ne!(red.fg, f.default_fg);
    assert_eq!(st(1), red, "same style shares a run");
    assert_eq!(row.runs[0].len, 2);

    let plain = st(2);
    assert_eq!(plain.fg, f.default_fg);
    assert_eq!(plain.bg, None);

    let inv = st(3);
    assert_eq!(inv.fg, f.default_bg, "inverse: fg takes the default bg");
    assert_eq!(
        inv.bg,
        Some(f.default_fg),
        "inverse: bg takes the default fg"
    );

    let ul = st(4);
    assert_eq!(
        (ul.flags & flags::UNDERLINE_MASK) >> flags::UNDERLINE_SHIFT,
        3
    );
    assert_eq!(ul.ul, Some(0x0000ff));

    assert_eq!(row_text(&f, 0), "abcde");
}

#[test]
fn background_only_cells_keep_their_color() {
    let mut e = emu();
    e.write(b"\x1b[44m\x1b[2K\x1b[0m");
    let f = e.build_frame(false).unwrap().unwrap();
    let r = &f.rows[0].runs[0];
    assert_eq!(r.len, 20);
    assert!(f.styles[r.s as usize].bg.is_some());
    assert!(r.text.is_none());
}

#[test]
fn wide_chars_mark_head_and_keep_tail_in_run() {
    let mut e = emu();
    e.write("a日b".as_bytes());
    let f = e.build_frame(false).unwrap().unwrap();
    let run = &f.rows[0].runs[0];
    let text = run.text.as_ref().unwrap();
    assert_eq!(&text[..4], ["a", "日", "", "b"]);
    assert_eq!(run.wide.as_deref(), Some(&[1u16][..]));
    assert_eq!(row_text(&f, 0), "a日b");
}

#[test]
fn cursor_on_wide_char_is_wide() {
    let mut e = emu();
    e.write("日本\x1b[1;1H".as_bytes());
    let f = e.build_frame(false).unwrap().unwrap();
    assert_eq!((f.cursor.x, f.cursor.y), (0, 0));
    assert!(f.cursor.wide);
}

#[test]
fn cursor_style_and_visibility() {
    let mut e = emu();
    e.build_frame(false).unwrap();
    e.write(b"\x1b[6 q");
    let f = e.build_frame(false).unwrap().unwrap();
    assert_eq!(f.cursor.style, CursorStyle::Bar);
    e.write(b"\x1b[?25l");
    let f = e.build_frame(false).unwrap().unwrap();
    assert!(!f.cursor.visible);
}

#[test]
fn mode_change_alone_sends_a_frame() {
    let mut e = emu();
    e.build_frame(false).unwrap();
    e.write(b"\x1b[?1003h\x1b[?1004h");
    let f = e
        .build_frame(false)
        .unwrap()
        .expect("mode change sends a frame");
    assert_eq!(f.mouse_tracking, MouseTracking::Any);
    assert!(f.focus_reporting);
}

#[test]
fn resize_sends_full_frame() {
    let mut e = emu();
    e.build_frame(false).unwrap();
    assert!(
        e.resize(Resize {
            cols: 30,
            rows: 8,
            cell_w: 10,
            cell_h: 20
        })
        .unwrap()
    );
    let f = e.build_frame(false).unwrap().unwrap();
    assert!(f.full);
    assert_eq!((f.cols, f.rows_total), (30, 8));
    assert_eq!(f.rows.len(), 8);
    assert!(
        !e.resize(Resize {
            cols: 30,
            rows: 8,
            cell_w: 10,
            cell_h: 20
        })
        .unwrap()
    );
}

#[test]
fn device_queries_get_answers() {
    let mut e = emu();
    e.write(b"\x1b[6n");
    assert_eq!(e.take_outbox(), b"\x1b[1;1R");
}

#[test]
fn render_hold_captures_the_frame_before_it() {
    let mut e = emu();
    e.build_frame(false).unwrap();
    e.write(b"before");
    e.write(b"\x1b[?2026h");
    assert!(e.held());
    let captured = e.take_captured().expect("frame captured at hold start");
    assert_eq!(row_text(&captured, 0), "before");

    e.write(b"\x1b[1;1Hduring");
    assert!(e.held());
    e.write(b"\x1b[?2026l");
    assert!(!e.held());
    let f = e.build_frame(false).unwrap().unwrap();
    assert_eq!(f.seq, captured.seq + 1);
    assert_eq!(row_text(&f, 0), "during");
}

#[test]
fn render_hold_times_out() {
    let mut e = emu();
    e.write(b"\x1b[?2026h");
    let d = e.hold_deadline().unwrap();
    assert!(!e.expire_hold(d - std::time::Duration::from_millis(1)));
    assert!(e.expire_hold(d));
    assert!(!e.held());
    assert!(
        !e.terminal()
            .mode(term::vt::mode::Mode::SYNC_OUTPUT)
            .unwrap()
    );
}

#[test]
fn keys_encode_by_mode() {
    let mut e = emu();
    assert_eq!(encode(&mut e, key("KeyA", Some("a"), 0)), b"a");
    assert_eq!(
        encode(&mut e, key("KeyA", Some("A"), proto::mods::SHIFT)),
        b"A"
    );
    assert_eq!(encode(&mut e, key("Enter", None, 0)), b"\r");
    assert_eq!(
        encode(&mut e, key("KeyC", None, proto::mods::CTRL)),
        b"\x03"
    );
    assert_eq!(encode(&mut e, key("ArrowUp", None, 0)), b"\x1b[A");
    e.write(b"\x1b[?1h");
    assert_eq!(encode(&mut e, key("ArrowUp", None, 0)), b"\x1bOA");
    let release = InputEvent::Key {
        action: KeyAction::Release,
        code: "KeyA".into(),
        key: "a".into(),
        text: None,
        mods: 0,
        composing: false,
    };
    assert_eq!(encode(&mut e, release), b"", "legacy mode drops releases");
}

#[test]
fn kitty_protocol_reports_releases() {
    let mut e = emu();
    // Disambiguate + report event types.
    e.write(b"\x1b[>3u");
    let release = InputEvent::Key {
        action: KeyAction::Release,
        code: "KeyA".into(),
        key: "a".into(),
        text: None,
        mods: 0,
        composing: false,
    };
    assert_eq!(encode(&mut e, release), b"\x1b[97;1:3u");
}

#[test]
fn text_and_paste() {
    let mut e = emu();
    assert_eq!(
        encode(
            &mut e,
            InputEvent::Text {
                data: "日本".into()
            }
        ),
        "日本".as_bytes()
    );
    assert_eq!(
        encode(
            &mut e,
            InputEvent::Paste {
                data: "a\nb".into()
            }
        ),
        b"a\rb"
    );
    e.write(b"\x1b[?2004h");
    assert_eq!(
        encode(
            &mut e,
            InputEvent::Paste {
                data: "a\nb".into()
            }
        ),
        b"\x1b[200~a\nb\x1b[201~"
    );
    let big = "x".repeat(10_000);
    assert_eq!(
        encode(&mut e, InputEvent::Paste { data: big.clone() }).len(),
        big.len() + 12
    );
}

#[test]
fn focus_only_when_reporting() {
    let mut e = emu();
    assert_eq!(encode(&mut e, InputEvent::Focus { focused: true }), b"");
    e.write(b"\x1b[?1004h");
    assert_eq!(
        encode(&mut e, InputEvent::Focus { focused: true }),
        b"\x1b[I"
    );
    assert_eq!(
        encode(&mut e, InputEvent::Focus { focused: false }),
        b"\x1b[O"
    );
}

#[test]
fn mouse_sgr() {
    let mut e = emu();
    let press = |x, y| InputEvent::Mouse {
        action: MouseAction::Press,
        button: Some(1),
        x,
        y,
        mods: 0,
    };
    assert_eq!(
        encode(&mut e, press(15.0, 45.0)),
        b"",
        "no tracking, no report"
    );
    e.write(b"\x1b[?1000h\x1b[?1006h");
    // Cell (1, 2) zero-based is reported as 2;3.
    assert_eq!(encode(&mut e, press(15.0, 45.0)), b"\x1b[<0;2;3M");
    let release = InputEvent::Mouse {
        action: MouseAction::Release,
        button: Some(1),
        x: 15.0,
        y: 45.0,
        mods: 0,
    };
    assert_eq!(encode(&mut e, release), b"\x1b[<0;2;3m");
}

#[test]
fn wheel_by_mode() {
    let mut e = emu();
    let wheel = |dy| InputEvent::Wheel {
        dx_rows: 0,
        dy_rows: dy,
        x: 5.0,
        y: 5.0,
        mods: 0,
    };
    assert_eq!(
        encode(&mut e, wheel(-2)),
        b"",
        "primary screen: nothing yet"
    );
    e.write(b"\x1b[?1049h");
    assert_eq!(
        encode(&mut e, wheel(-2)),
        b"\x1b[A\x1b[A",
        "alternate scroll"
    );
    e.write(b"\x1b[?1000h\x1b[?1006h");
    assert_eq!(encode(&mut e, wheel(1)), b"\x1b[<65;1;1M");
    assert_eq!(
        encode(&mut e, wheel(-2)),
        b"\x1b[<64;1;1M\x1b[<64;1;1M",
        "one press per row"
    );
}

// ---- selection and scrollback -------------------------------------------

fn mouse(action: MouseAction, col: f32, row: f32, mods: u16) -> InputEvent {
    // Middle of the cell, with SIZE's 10×20 px cells.
    InputEvent::Mouse {
        action,
        button: Some(1),
        x: col * 10.0 + 5.0,
        y: row * 20.0 + 10.0,
        mods,
    }
}

fn click(e: &mut Emulator, col: f32, row: f32) {
    e.input(&mouse(MouseAction::Press, col, row, 0)).unwrap();
    e.input(&mouse(MouseAction::Release, col, row, 0)).unwrap();
}

/// Drag from the left part of one cell to the right part of another; a cell
/// is included once the pointer passes its middle, as in Ghostty.
fn drag(e: &mut Emulator, from: (f32, f32), to: (f32, f32), mods: u16) {
    e.input(&mouse(MouseAction::Press, from.0 - 0.3, from.1, mods))
        .unwrap();
    e.input(&mouse(MouseAction::Motion, to.0 + 0.3, to.1, mods))
        .unwrap();
    e.input(&mouse(MouseAction::Release, to.0 + 0.3, to.1, mods))
        .unwrap();
}

fn style_at(f: &Frame, x: u16, y: u16) -> proto::Style {
    let row = f.rows.iter().find(|r| r.y == y).expect("row in frame");
    let run = row
        .runs
        .iter()
        .find(|r| x >= r.x && x < r.x + r.len)
        .unwrap();
    f.styles[run.s as usize]
}

#[test]
fn drag_selects_and_reports_text() {
    let mut e = emu();
    e.write(b"hello world");
    e.build_frame(false).unwrap();
    drag(&mut e, (0.0, 0.0), (4.0, 0.0), 0);
    let f = e.build_frame(false).unwrap().unwrap();
    assert_eq!(f.selection_text.as_deref(), Some("hello"));
    let sel = style_at(&f, 0, 0);
    assert_eq!(
        (sel.fg, sel.bg),
        (f.default_bg, Some(f.default_fg)),
        "selection swaps the window colors"
    );
    assert_eq!(style_at(&f, 4, 0), sel);
    assert_eq!(style_at(&f, 6, 0).bg, None, "outside the selection");
    // Nothing changed: no text resent.
    assert!(e.build_frame(false).unwrap().is_none());
}

#[test]
fn double_and_triple_click() {
    let mut e = emu();
    e.write(b"hello world");
    click(&mut e, 7.0, 0.0);
    click(&mut e, 7.0, 0.0);
    let f = e.build_frame(false).unwrap().unwrap();
    assert_eq!(f.selection_text.as_deref(), Some("world"));
    click(&mut e, 7.0, 0.0);
    let f = e.build_frame(false).unwrap().unwrap();
    assert_eq!(f.selection_text.as_deref(), Some("hello world"));
}

#[test]
fn wrapped_lines_copy_as_one() {
    let mut e = emu();
    e.write(b"0123456789abcdefghijKLMNO\r\nnext");
    drag(&mut e, (0.0, 0.0), (4.0, 1.0), 0);
    let f = e.build_frame(false).unwrap().unwrap();
    assert_eq!(
        f.selection_text.as_deref(),
        Some("0123456789abcdefghijKLMNO")
    );
}

#[test]
fn rectangle_with_alt() {
    let mut e = emu();
    e.write(b"abcdef\r\nghijkl\r\nmnopqr");
    drag(&mut e, (1.0, 0.0), (2.0, 2.0), proto::mods::ALT);
    let f = e.build_frame(false).unwrap().unwrap();
    assert_eq!(f.selection_text.as_deref(), Some("bc\nhi\nno"));
}

#[test]
fn click_and_typing_clear_the_selection() {
    let mut e = emu();
    e.write(b"hello world");
    drag(&mut e, (0.0, 0.0), (4.0, 0.0), 0);
    e.build_frame(false).unwrap();
    click(&mut e, 8.0, 0.0);
    let f = e.build_frame(false).unwrap().unwrap();
    assert_eq!(f.selection_text.as_deref(), Some(""));
    assert_eq!(style_at(&f, 0, 0).bg, None);

    drag(&mut e, (0.0, 0.0), (4.0, 0.0), 0);
    e.build_frame(false).unwrap();
    assert_eq!(encode(&mut e, key("KeyX", Some("x"), 0)), b"x");
    let f = e.build_frame(false).unwrap().unwrap();
    assert_eq!(f.selection_text.as_deref(), Some(""));
}

#[test]
fn modifier_keys_keep_the_selection() {
    let mut e = emu();
    e.write(b"hello");
    drag(&mut e, (0.0, 0.0), (4.0, 0.0), 0);
    e.build_frame(false).unwrap();
    encode(&mut e, key("MetaLeft", None, proto::mods::SUPER));
    assert!(
        e.build_frame(false).unwrap().is_none(),
        "selection unchanged"
    );
}

#[test]
fn tracking_programs_get_clicks_unless_shift() {
    let mut e = emu();
    e.write(b"hello\x1b[?1000h\x1b[?1006h");
    e.build_frame(false).unwrap();
    assert_eq!(
        encode(&mut e, mouse(MouseAction::Press, 1.0, 0.0, 0)),
        b"\x1b[<0;2;1M"
    );
    assert_eq!(
        encode(&mut e, mouse(MouseAction::Release, 1.0, 0.0, 0)),
        b"\x1b[<0;2;1m"
    );
    assert!(e.build_frame(false).unwrap().is_none(), "no selection");

    let shift = proto::mods::SHIFT;
    assert_eq!(
        encode(&mut e, mouse(MouseAction::Press, -0.3, 0.0, shift)),
        b""
    );
    assert_eq!(
        encode(&mut e, mouse(MouseAction::Motion, 4.3, 0.0, shift)),
        b""
    );
    assert_eq!(
        encode(&mut e, mouse(MouseAction::Release, 4.3, 0.0, shift)),
        b""
    );
    let f = e.build_frame(false).unwrap().unwrap();
    assert_eq!(f.selection_text.as_deref(), Some("hello"));
}

/// 30 numbered lines on a 5-row screen: 26 go to scrollback.
fn with_history() -> Emulator {
    let mut e = emu();
    for i in 1..=30 {
        e.write(format!("line {i}\r\n").as_bytes());
    }
    e
}

#[test]
fn wheel_and_scroll_move_through_scrollback() {
    let mut e = with_history();
    let f = e.build_frame(false).unwrap().unwrap();
    let bottom = f.scrollbar;
    assert_eq!(bottom.len, 5);
    assert_eq!(
        bottom.offset + bottom.len,
        bottom.total,
        "starts at the bottom"
    );
    assert_eq!(row_text(&f, 3), "line 30");

    let wheel = InputEvent::Wheel {
        dx_rows: 0,
        dy_rows: -3,
        x: 5.0,
        y: 5.0,
        mods: 0,
    };
    assert_eq!(encode(&mut e, wheel), b"");
    let f = e.build_frame(false).unwrap().unwrap();
    assert_eq!(f.scrollbar.offset, bottom.offset - 3);
    assert_eq!(row_text(&f, 3), "line 27");

    e.input(&InputEvent::Scroll {
        by: proto::ScrollBy::Top,
        n: 0,
    })
    .unwrap();
    let f = e.build_frame(false).unwrap().unwrap();
    assert_eq!(f.scrollbar.offset, 0);
    assert_eq!(row_text(&f, 0), "line 1");

    e.input(&InputEvent::Scroll {
        by: proto::ScrollBy::Pages,
        n: 1,
    })
    .unwrap();
    let f = e.build_frame(false).unwrap().unwrap();
    assert_eq!(row_text(&f, 0), "line 6");

    // Typing jumps back to the bottom.
    encode(&mut e, key("KeyA", Some("a"), 0));
    let f = e.build_frame(false).unwrap().unwrap();
    assert_eq!(f.scrollbar, bottom);
}

#[test]
fn scrolled_back_view_stays_put_while_output_arrives() {
    let mut e = with_history();
    e.input(&InputEvent::Scroll {
        by: proto::ScrollBy::Top,
        n: 0,
    })
    .unwrap();
    e.build_frame(false).unwrap();
    e.write(b"more output\r\n");
    if let Some(f) = e.build_frame(false).unwrap() {
        assert!(f.rows.iter().all(|r| r.y != 0) || row_text(&f, 0) == "line 1");
        assert_eq!(f.scrollbar.offset, 0, "still at the top");
    }
}

#[test]
fn selection_in_scrollback_copies_history() {
    let mut e = with_history();
    e.input(&InputEvent::Scroll {
        by: proto::ScrollBy::Top,
        n: 0,
    })
    .unwrap();
    drag(&mut e, (0.0, 0.0), (5.0, 1.0), 0);
    let f = e.build_frame(false).unwrap().unwrap();
    assert_eq!(f.selection_text.as_deref(), Some("line 1\nline 2"));
}

#[test]
fn dragging_past_the_top_autoscrolls() {
    let mut e = with_history();
    e.build_frame(false).unwrap();
    let start = e.terminal().scrollbar().unwrap().offset;
    e.input(&mouse(MouseAction::Press, 3.0, 2.0, 0)).unwrap();
    e.input(&InputEvent::Mouse {
        action: MouseAction::Motion,
        button: Some(1),
        x: 30.0,
        y: -15.0,
        mods: 0,
    })
    .unwrap();
    assert!(e.autoscrolling());
    for _ in 0..4 {
        e.autoscroll_tick().unwrap();
    }
    assert_eq!(e.terminal().scrollbar().unwrap().offset, start - 4);
    e.input(&InputEvent::Mouse {
        action: MouseAction::Release,
        button: Some(1),
        x: 30.0,
        y: -15.0,
        mods: 0,
    })
    .unwrap();
    assert!(!e.autoscrolling());
    let f = e.build_frame(false).unwrap().unwrap();
    let text = f.selection_text.unwrap();
    assert!(text.lines().count() >= 6, "selection grew upward: {text:?}");
}

// ---- links --------------------------------------------------------------

fn hover(e: &mut Emulator, col: f32, row: f32, on: bool) -> Option<proto::Link> {
    e.input(&InputEvent::Hover {
        x: col * 10.0 + 5.0,
        y: row * 20.0 + 10.0,
        on,
    })
    .unwrap();
    e.build_frame(false).unwrap().and_then(|f| f.link)
}

fn span(y: u16, x: u16, len: u16) -> proto::LinkSpan {
    proto::LinkSpan { y, x, len }
}

#[test]
fn a_wrapped_url_is_one_link_on_every_row() {
    let mut e = emu();
    // 20 columns: the URL runs over rows 0, 1 and 2.
    e.write(b"go https://example.com/a/b/c/d/e/f/g/h/i/j. ok");
    e.build_frame(false).unwrap();
    let link = hover(&mut e, 5.0, 1.0, true).expect("link");
    assert_eq!(link.uri, "https://example.com/a/b/c/d/e/f/g/h/i/j");
    assert_eq!(link.target, link.uri);
    assert_eq!(
        link.spans,
        vec![span(0, 3, 17), span(1, 0, 20), span(2, 0, 2)]
    );

    // Same link: nothing new to send.
    assert_eq!(hover(&mut e, 6.0, 1.0, true), None);
    // Off the link, then the modifier released: cleared once.
    assert_eq!(
        hover(&mut e, 4.0, 2.0, true).map(|l| l.uri),
        Some(String::new())
    );
    hover(&mut e, 5.0, 1.0, true).expect("link again");
    assert_eq!(
        hover(&mut e, 5.0, 1.0, false).map(|l| l.uri),
        Some(String::new())
    );
}

#[test]
fn hidden_links_open_their_address() {
    let mut e = emu();
    e.write(b"see \x1b]8;;https://a.example/x\x1b\\the docs\x1b]8;;\x1b\\ now");
    e.build_frame(false).unwrap();
    let link = hover(&mut e, 9.0, 0.0, true).expect("link");
    assert_eq!(link.uri, "https://a.example/x");
    assert_eq!(link.target, "https://a.example/x");
    assert_eq!(link.spans, vec![span(0, 4, 8)]);
    // "now" is plain text.
    assert_eq!(
        hover(&mut e, 14.0, 0.0, true).map(|l| l.uri),
        Some(String::new())
    );
}

#[test]
fn paths_resolve_from_the_pane_directory() {
    let dir = std::env::temp_dir().join(format!("kibann-emu-links-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.md"), "").unwrap();
    let mut e = emu();
    e.set_cwd(Some(dir.clone()));
    e.write(b"at notes.md:3: no\r\nmissing.md");
    e.build_frame(false).unwrap();
    let link = hover(&mut e, 4.0, 0.0, true).expect("link");
    assert_eq!(link.uri, "notes.md:3");
    assert_eq!(link.target, dir.join("notes.md").to_string_lossy());
    assert_eq!(link.spans, vec![span(0, 3, 10)]);
    // A name that doesn't exist isn't a link.
    assert_eq!(
        hover(&mut e, 2.0, 1.0, true).map(|l| l.uri),
        Some(String::new())
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_url_starting_in_scrollback_keeps_its_start() {
    let mut e = emu();
    // 50 characters over three rows, then two more lines push the first
    // URL row above the 5-row viewport.
    e.write(b"https://example.com/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n1\r\n2\r\n3");
    e.build_frame(false).unwrap();
    let link = hover(&mut e, 2.0, 1.0, true).expect("link");
    assert_eq!(
        link.uri,
        "https://example.com/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    assert_eq!(link.spans, vec![span(0, 0, 20), span(1, 0, 11)]);
}

#[test]
fn hovering_never_reaches_the_program() {
    let mut e = emu();
    e.write(b"\x1b[?1003h\x1b[?1006hhttps://x.example");
    let out = encode(
        &mut e,
        InputEvent::Hover {
            x: 15.0,
            y: 10.0,
            on: true,
        },
    );
    assert!(out.is_empty());
}

// ---- reading text for agents ----

fn read(e: &Emulator, source: proto::wire::ReadSource, lines: Option<usize>) -> String {
    term::read::text(e.terminal(), e.size(), source, lines).unwrap()
}

#[test]
fn reading_the_screen_and_the_scrollback() {
    use proto::wire::ReadSource::{Recent, Visible};
    let mut e = emu();
    for i in 1..=8 {
        e.write(format!("line {i}\r\n").as_bytes());
    }
    // A line wider than the screen wraps onto two rows.
    e.write(b"0123456789abcdefghijKLMNO");
    // The screen holds the last 5 rows: 6, 7, 8 and the wrapped line.
    assert_eq!(
        read(&e, Visible, None),
        "line 6\nline 7\nline 8\n0123456789abcdefghij\nKLMNO"
    );
    // Scrollback and screen, the wrapped line joined.
    let recent = read(&e, Recent, None);
    assert!(recent.starts_with("line 1\nline 2\n"), "{recent}");
    assert!(
        recent.ends_with("line 8\n0123456789abcdefghijKLMNO"),
        "{recent}"
    );
    assert_eq!(
        read(&e, Recent, Some(2)),
        "line 8\n0123456789abcdefghijKLMNO"
    );
}

#[test]
fn reading_a_mostly_empty_screen() {
    use proto::wire::ReadSource::{Recent, Visible};
    let mut e = emu();
    e.write(b"$ ");
    assert_eq!(read(&e, Visible, None), "$");
    assert_eq!(read(&e, Recent, None), "$");
}

#[test]
fn a_theme_sets_the_colors_programs_and_frames_see() {
    let mut e = emu();
    let mut t = proto::theme::Theme::kibann_light();
    t.palette[1] = 0x112233;
    e.set_theme(&t).unwrap();
    e.write(b"\x1b[31mr\x1b[0m");
    let f = e.build_frame(true).unwrap().unwrap();
    assert_eq!(f.default_bg, 0xffffff);
    assert_eq!(f.default_fg, 0x1f2328);
    assert_eq!(f.styles[f.rows[0].runs[0].s as usize].fg, 0x112233);
    assert_eq!(f.cursor.color, Some(0x0969da));

    // A program asking for the background (OSC 11) gets the theme's.
    e.write(b"\x1b]11;?\x1b\\");
    let reply = String::from_utf8(e.take_outbox()).unwrap();
    assert!(reply.contains("rgb:ffff/ffff/ffff"), "{reply:?}");

    // A color the program set itself survives a theme change.
    e.write(b"\x1b]11;rgb:12/34/56\x1b\\");
    e.set_theme(&proto::theme::Theme::kibann_dark()).unwrap();
    let f = e.build_frame(true).unwrap().unwrap();
    assert_eq!(f.default_bg, 0x123456);
    assert_eq!(f.default_fg, 0xffffff);
}

#[test]
fn a_theme_colors_the_selection() {
    let mut e = emu();
    let mut t = proto::theme::Theme::kibann_dark();
    t.selection_bg = Some(0x334455);
    t.selection_fg = None;
    e.set_theme(&t).unwrap();
    e.write(b"hello");
    // Select "hel" by dragging.
    let at = |x: f32| InputEvent::Mouse {
        action: MouseAction::Press,
        button: Some(1),
        x,
        y: 5.0,
        mods: 0,
    };
    e.input(&at(1.0)).unwrap();
    e.input(&InputEvent::Mouse {
        action: MouseAction::Motion,
        button: Some(1),
        x: 29.0,
        y: 5.0,
        mods: 0,
    })
    .unwrap();
    let f = e.build_frame(true).unwrap().unwrap();
    let run = &f.rows[0].runs[0];
    let s = f.styles[run.s as usize];
    assert_eq!(s.bg, Some(0x334455));
    assert_eq!(
        s.fg, 0xffffff,
        "the text keeps its color without selection-foreground"
    );
}
