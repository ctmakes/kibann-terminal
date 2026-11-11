use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use proto::{Frame, InputEvent, Resize};
use term::{Event, Session};

const SIZE: Resize = Resize {
    cols: 20,
    rows: 5,
    cell_w: 10,
    cell_h: 20,
};
const WAIT: Duration = Duration::from_secs(5);

/// The fake PTY's output end: reads block until the test sends bytes.
struct ChanReader {
    rx: Receiver<Vec<u8>>,
    pending: Vec<u8>,
}

impl Read for ChanReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pending.is_empty() {
            match self.rx.recv() {
                Ok(d) => self.pending = d,
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }
}

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Row 0's text, blanks as spaces.
fn line0(f: &Frame) -> String {
    let row = f.rows.iter().find(|r| r.y == 0).expect("row 0");
    let mut s = String::new();
    for run in &row.runs {
        match &run.text {
            Some(t) => t
                .iter()
                .for_each(|c| s.push_str(if c.is_empty() { " " } else { c })),
            None => s.extend(std::iter::repeat_n(' ', run.len as usize)),
        }
    }
    s.trim_end().to_owned()
}

struct Harness {
    session: Session,
    pty: Sender<Vec<u8>>,
    written: Captured,
    frames: Receiver<Frame>,
}

fn harness() -> Harness {
    let (pty, rx) = mpsc::channel();
    let written = Captured::default();
    let session = Session::with_io(
        SIZE,
        ChanReader {
            rx,
            pending: Vec::new(),
        },
        written.clone(),
    )
    .unwrap();
    let (ftx, frames) = mpsc::channel();
    session.attach(move |e| {
        if let Event::Frame(f) = e {
            let _ = ftx.send(f);
        }
    });
    Harness {
        session,
        pty,
        written,
        frames,
    }
}

impl Harness {
    fn next(&self) -> Frame {
        self.frames.recv_timeout(WAIT).expect("a frame")
    }
    fn none_within(&self, d: Duration) -> bool {
        self.frames.recv_timeout(d).is_err()
    }
}

#[test]
fn attach_sends_full_frame() {
    let h = harness();
    let f = h.next();
    assert!(f.full);
    assert_eq!(f.rows.len(), 5);
}

#[test]
fn output_produces_frames_and_backpressure_holds_them() {
    let h = harness();
    let f1 = h.next(); // full, unacked: 1
    h.pty.send(b"one".to_vec()).unwrap();
    let f2 = h.next(); // unacked: 2
    assert_eq!(f2.seq, f1.seq + 1);

    // Two frames unacknowledged: changes accumulate but nothing is sent.
    h.pty.send(b" two".to_vec()).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    h.pty.send(b" three".to_vec()).unwrap();
    assert!(h.none_within(Duration::from_millis(100)));

    h.session.ack(f2.seq);
    let f3 = h.next();
    assert_eq!(f3.seq, f2.seq + 1);
    assert_eq!(
        line0(&f3),
        "one two three",
        "the next frame carries everything"
    );
}

#[test]
fn frames_are_throttled() {
    let h = harness();
    let f = h.next();
    h.session.ack(f.seq);
    let start = std::time::Instant::now();
    let mut seqs = Vec::new();
    for i in 0..10 {
        h.pty.send(format!("{i}").into_bytes()).unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    // Ack as frames arrive and collect what came in ~60 ms.
    while start.elapsed() < Duration::from_millis(60) {
        if let Ok(f) = h.frames.recv_timeout(Duration::from_millis(5)) {
            seqs.push(f.seq);
            h.session.ack(f.seq);
        }
    }
    assert!(!seqs.is_empty());
    assert!(
        seqs.len() <= 5,
        "ten writes in 20 ms should not make {} frames",
        seqs.len()
    );
}

#[test]
fn request_full_and_reattach() {
    let h = harness();
    let f = h.next();
    h.session.ack(f.seq);
    h.session.request_full();
    let f = h.next();
    assert!(f.full);

    let (tx, rx) = mpsc::channel();
    h.session.attach(move |e| {
        if let Event::Frame(f) = e {
            let _ = tx.send(f);
        }
    });
    let f2 = rx.recv_timeout(WAIT).unwrap();
    assert!(f2.full);
    assert!(f2.seq > f.seq, "seq keeps increasing across attaches");
}

#[test]
fn input_and_device_responses_reach_the_pty() {
    let h = harness();
    h.next();
    h.session.input(InputEvent::Text {
        data: "ls\r".into(),
    });
    h.pty.send(b"\x1b[6n".to_vec()).unwrap();
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        let w = h.written.0.lock().unwrap().clone();
        if w.windows(6).any(|x| x == b"\x1b[1;1R") && w.starts_with(b"ls\r") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "got {:?}",
            String::from_utf8_lossy(&w)
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn resize_sends_full_frame() {
    let h = harness();
    let f = h.next();
    h.session.ack(f.seq);
    h.session.resize(Resize {
        cols: 40,
        rows: 10,
        cell_w: 10,
        cell_h: 20,
    });
    let f = h.next();
    assert!(f.full);
    assert_eq!((f.cols, f.rows_total), (40, 10));
}

#[test]
fn stuck_render_hold_releases_after_timeout() {
    let h = harness();
    let f = h.next();
    h.session.ack(f.seq);
    h.pty.send(b"\x1b[?2026hstuck".to_vec()).unwrap();
    // Nothing changed before the hold began, so no capture goes out; the
    // held text shows up once the hold times out (1 s).
    let start = std::time::Instant::now();
    let f = loop {
        let f = h.next();
        h.session.ack(f.seq);
        if !f.rows.is_empty() {
            break f;
        }
    };
    assert!(
        start.elapsed() >= Duration::from_millis(900),
        "released after {:?}",
        start.elapsed()
    );
    assert_eq!(line0(&f), "stuck");
}

#[cfg(unix)]
#[test]
fn real_pty_runs_a_command_and_reports_exit() {
    let session = Session::spawn(
        term::Spawn {
            argv: Some(vec![
                "/bin/sh".into(),
                "-c".into(),
                "printf 'hi from pty'; exit 3".into(),
            ]),
            ..Default::default()
        },
        SIZE,
    )
    .unwrap();
    let (tx, rx) = mpsc::channel();
    session.attach(move |e| {
        let _ = tx.send(e);
    });
    let mut seen = String::new();
    loop {
        match rx.recv_timeout(WAIT).expect("event") {
            Event::Frame(f) => {
                session.ack(f.seq);
                if f.rows.iter().any(|r| r.y == 0) {
                    seen = line0(&f);
                }
            }
            Event::Exited(code) => {
                assert_eq!(code, Some(3));
                break;
            }
        }
    }
    assert!(seen.starts_with("hi from pty"), "row 0 was {seen:?}");
}
