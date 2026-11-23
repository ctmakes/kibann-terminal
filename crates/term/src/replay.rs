//! Play a recording through the emulator and the session's own pacing
//! ([`crate::pacer`]), on the recording's clock instead of the wall clock.
//! Backpressure is left out: the frontend acks every frame at once.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use proto::{Frame, Resize};
use serde::{Deserialize, Serialize};

use crate::cast::{CastEvent, Header};
use crate::emulator::Emulator;
use crate::pacer::Pacer;

/// A frame and when it would have been sent, in ms from the start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimedFrame {
    pub t: f64,
    pub frame: Frame,
}

/// Default cell size in device px. It only affects pixel size reports.
pub const CELL: (u16, u16) = (8, 16);

/// The recording's clock: ms from its start, as the [`Instant`]s the pacer
/// works in.
struct Clock(Instant);

impl Clock {
    fn at(&self, ms: f64) -> Instant {
        self.0 + Duration::from_nanos((ms * 1e6).round() as u64)
    }

    fn ms(&self, t: Instant) -> f64 {
        (t - self.0).as_nanos() as f64 / 1e6
    }
}

struct Player {
    emu: Emulator,
    pacer: Pacer,
    clock: Clock,
    /// Frames the pacer sent and haven't been timed yet.
    sent: Arc<Mutex<Vec<Frame>>>,
    out: Vec<TimedFrame>,
    /// When the pacer wants to be back.
    wake: Option<Instant>,
}

impl Player {
    /// What the session does at `t`; what it sent goes out at `t`, acked.
    fn tick(&mut self, t: Instant) {
        self.wake = self.pacer.tick(&mut self.emu, t);
        let ms = self.clock.ms(t);
        for frame in self.sent.lock().unwrap().drain(..) {
            self.pacer.ack(0, frame.seq);
            self.out.push(TimedFrame { t: ms, frame });
        }
    }

    /// Everything the session would have done on its own before `t`.
    fn until(&mut self, t: Instant) {
        while let Some(w) = self.wake.filter(|w| *w <= t) {
            self.tick(w);
        }
    }
}

/// Replay `events` and return every frame a session would have sent.
pub fn replay(
    header: Header,
    events: &[CastEvent],
    cell: (u16, u16),
) -> Result<Vec<TimedFrame>, String> {
    let size = |cols, rows| Resize {
        cols,
        rows,
        cell_w: cell.0,
        cell_h: cell.1,
    };
    let emu = Emulator::new(size(header.width, header.height)).map_err(|e| e.to_string())?;
    let sent = Arc::<Mutex<Vec<Frame>>>::default();
    let mut pacer = Pacer::default();
    let to = sent.clone();
    pacer.add_view(0, Box::new(move |f| to.lock().unwrap().push(f)));
    let clock = Clock(Instant::now());
    let start = clock.at(0.0);
    let mut p = Player {
        emu,
        pacer,
        clock,
        sent,
        out: Vec::new(),
        wake: None,
    };
    p.tick(start);

    for ev in events {
        let t = p.clock.at(ev.time() * 1e3);
        p.until(t);
        match ev {
            CastEvent::Output(_, data) => {
                p.emu.write(data.as_bytes());
                p.pacer.changed();
            }
            CastEvent::Resize(_, cols, rows) => {
                if p.emu
                    .resize(size(*cols, *rows))
                    .map_err(|e| e.to_string())?
                {
                    p.pacer.changed_all();
                }
            }
            CastEvent::Input(..) => {}
        }
        p.tick(t);
    }
    // Let the last hold time out and the last frame go out.
    while let Some(w) = p.wake {
        p.tick(w);
    }
    Ok(p.out)
}

/// Frames as JSON lines.
pub fn to_jsonl(frames: &[TimedFrame]) -> String {
    let mut s = String::new();
    for f in frames {
        s.push_str(&serde_json::to_string(f).expect("frames serialize"));
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: Header = Header {
        width: 20,
        height: 4,
    };

    fn out(t: f64, s: &str) -> CastEvent {
        CastEvent::Output(t, s.into())
    }

    #[test]
    fn paces_frames_on_the_recording_clock() {
        // Writes at 0.1 s, 0.105 s and 0.110 s: the first goes out at once,
        // the other two together at the 16 ms mark.
        let ev = [out(0.1, "a"), out(0.105, "b"), out(0.110, "c")];
        let f = replay(H, &ev, CELL).unwrap();
        let times: Vec<f64> = f.iter().map(|f| f.t).collect();
        assert_eq!(times, vec![0.0, 100.0, 116.0]);
        assert!(f[0].frame.full);
        assert_eq!(
            f.iter().map(|f| f.frame.seq).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn holds_show_the_captured_frame_then_the_result() {
        // "ld" is still waiting for the 16 ms mark when the hold begins, so
        // the hold captures it.
        let ev = [
            out(0.1, "o"),
            out(0.105, "ld"),
            out(0.11, "\x1b[?2026h\x1b[1;1Hnew"),
            out(0.25, "\x1b[?2026l"),
        ];
        let f = replay(H, &ev, CELL).unwrap();
        let text =
            |i: usize| -> String { f[i].frame.rows[0].runs[0].text.as_ref().unwrap().concat() };
        assert_eq!(f.len(), 4, "{f:?}");
        assert_eq!(text(1), "o");
        assert_eq!((f[2].t, text(2)), (110.0, "old".into()));
        assert_eq!((f[3].t, text(3)), (250.0, "new".into()));
    }

    #[test]
    fn stuck_hold_times_out() {
        let ev = [out(0.1, "\x1b[?2026hx")];
        let f = replay(H, &ev, CELL).unwrap();
        let last = f.last().unwrap();
        assert_eq!(last.t, 1100.0);
        assert_eq!(last.frame.rows[0].runs[0].text.as_ref().unwrap()[0], "x");
    }

    #[test]
    fn resize_gives_a_full_frame() {
        let ev = [CastEvent::Resize(0.5, 30, 6)];
        let f = replay(H, &ev, CELL).unwrap();
        assert!(f[1].frame.full);
        assert_eq!((f[1].frame.cols, f[1].frame.rows_total), (30, 6));
    }
}
