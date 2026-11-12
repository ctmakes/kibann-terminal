//! Play a recording through the emulator and frame builder, paced the way a
//! live session paces frames, but on the recording's clock instead of the
//! wall clock. Backpressure is left out: the frontend is assumed to keep up.

use proto::{Frame, Resize};
use serde::{Deserialize, Serialize};

use crate::cast::{CastEvent, Header};
use crate::emulator::{Emulator, HOLD_TIMEOUT};
use crate::session::FRAME_INTERVAL;

/// A frame and when it would have been sent, in ms from the start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimedFrame {
    pub t: f64,
    pub frame: Frame,
}

/// Default cell size in device px. It only affects pixel size reports.
pub const CELL: (u16, u16) = (8, 16);

struct Player {
    emu: Emulator,
    out: Vec<TimedFrame>,
    last: f64,
    /// A frame is waiting for the 16 ms mark.
    pending: bool,
    hold_start: Option<f64>,
}

impl Player {
    fn push(&mut self, t: f64, frame: Option<Frame>) {
        if let Some(frame) = frame {
            self.last = t;
            self.out.push(TimedFrame { t, frame });
        }
    }

    fn build(&mut self, t: f64, full: bool) -> Result<(), String> {
        let f = self.emu.build_frame(full).map_err(|e| e.to_string())?;
        self.pending = false;
        self.push(t, f);
        Ok(())
    }

    /// Do whatever the session would have done on its own before time `t`:
    /// time out a hold, send a frame that came due.
    fn advance(&mut self, t: f64) -> Result<(), String> {
        let interval = FRAME_INTERVAL.as_secs_f64() * 1e3;
        if let Some(h) = self.hold_start {
            let end = h + HOLD_TIMEOUT.as_secs_f64() * 1e3;
            if t < end {
                return Ok(());
            }
            self.emu.end_hold();
            self.hold_start = None;
            let at = end.max(self.last + interval);
            if at <= t {
                self.build(at, false)?;
            } else {
                self.pending = true;
            }
        }
        if self.pending && t >= self.last + interval {
            self.build(self.last + interval, false)?;
        }
        Ok(())
    }

    /// After feeding output at `t`: send a captured frame, track holds, and
    /// send now if the leading edge allows it.
    fn after_output(&mut self, t: f64) -> Result<(), String> {
        let captured = self.emu.take_captured();
        self.push(t, captured);
        if self.emu.held() {
            self.hold_start.get_or_insert(t);
            return Ok(());
        }
        self.hold_start = None;
        if t >= self.last + FRAME_INTERVAL.as_secs_f64() * 1e3 {
            self.build(t, false)
        } else {
            self.pending = true;
            Ok(())
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
    let mut p = Player {
        emu,
        out: Vec::new(),
        last: 0.0,
        pending: false,
        hold_start: None,
    };
    p.build(0.0, true)?;

    for ev in events {
        let t = ev.time() * 1e3;
        p.advance(t)?;
        match ev {
            CastEvent::Output(_, data) => {
                p.emu.write(data.as_bytes());
                p.after_output(t)?;
            }
            CastEvent::Resize(_, cols, rows) => {
                if p.emu
                    .resize(size(*cols, *rows))
                    .map_err(|e| e.to_string())?
                {
                    p.build(t, true)?;
                }
            }
            CastEvent::Input(..) => {}
        }
    }
    // Let the last hold time out and the last frame go out.
    p.advance(f64::INFINITY)?;
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
