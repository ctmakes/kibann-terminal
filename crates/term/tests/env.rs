//! The shell doesn't inherit variables describing the terminal that started
//! the app. Its own binary: it sets process environment variables.

#![cfg(unix)]

use std::sync::mpsc;
use std::time::Duration;

use proto::Resize;
use term::session::is_foreign_terminal_var;
use term::{Event, Session, Spawn};

#[test]
fn foreign_terminal_variables_are_dropped() {
    // SAFETY: the only test in this binary, before any thread starts.
    unsafe {
        std::env::set_var("TERM_PROGRAM", "ghostty");
        std::env::set_var("GHOSTTY_RESOURCES_DIR", "/x");
        std::env::set_var("TMUX", "/tmp/tmux,1,0");
        std::env::set_var("EDITOR", "vim");
    }
    let session = Session::spawn(
        Spawn {
            argv: Some(vec![
                "/bin/sh".into(),
                "-c".into(),
                r#"printf '%s|%s|%s|%s' "$TERM_PROGRAM" "${GHOSTTY_RESOURCES_DIR-unset}" "${TMUX-unset}" "$EDITOR""#
                    .into(),
            ]),
            ..Default::default()
        },
        Resize { cols: 60, rows: 3, cell_w: 8, cell_h: 16 },
    )
    .unwrap();
    let (tx, rx) = mpsc::channel();
    session.attach(move |e| {
        let _ = tx.send(e);
    });
    let mut line = String::new();
    while let Event::Frame(f) = rx.recv_timeout(Duration::from_secs(5)).expect("event") {
        session.ack(f.seq);
        if let Some(row) = f.rows.iter().find(|r| r.y == 0) {
            line = row
                .runs
                .iter()
                .flat_map(|r| r.text.clone().unwrap_or_default())
                .collect::<String>();
        }
    }
    assert_eq!(line.trim_end(), "kibann|unset|unset|vim");

    assert!(is_foreign_terminal_var("KITTY_WINDOW_ID"));
    assert!(is_foreign_terminal_var("VSCODE_INJECTION"));
    assert!(!is_foreign_terminal_var("EDITOR"));
    assert!(!is_foreign_terminal_var("TERM"));
    assert!(!is_foreign_terminal_var("PATH"));
}
