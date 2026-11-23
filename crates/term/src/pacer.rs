//! When frames go out, for every view of a session. The session thread and
//! [`crate::replay`] both drive it: one on the wall clock, the other on a
//! recording's.
//!
//! A frame goes out right away if none went out in the last
//! [`FRAME_INTERVAL`], otherwise at that mark. Nothing is built for a view
//! with more than one frame unacknowledged; changes keep accumulating and
//! its next frame carries all of them. Frames are built once for all
//! views: a view still waiting on acks misses one and gets a full frame
//! once it catches up, so a slow view never holds back a fast one.
//!
//! While a program holds the screen (synchronized output, mode 2026) the
//! frame captured as the hold began goes out and nothing else, until the
//! program lets go or [`HOLD_TIMEOUT`] passes.

use std::time::{Duration, Instant};

use proto::Frame;

use crate::emulator::Emulator;

/// Minimum time between frames.
pub const FRAME_INTERVAL: Duration = Duration::from_millis(16);
/// How long a program may hold the screen (mode 2026) before it shows
/// anyway.
pub const HOLD_TIMEOUT: Duration = Duration::from_secs(1);

/// Where one view's frames go.
pub type FrameSink = Box<dyn FnMut(Frame) + Send>;
/// Names a view of a session; the caller picks it.
pub type ViewId = u64;

struct View {
    id: ViewId,
    sink: FrameSink,
    /// Seq of the last frame sent to it and the last one it acknowledged.
    sent: u64,
    acked: u64,
    /// It has every frame built so far, so the next partial one applies.
    current: bool,
}

impl View {
    fn ready(&self) -> bool {
        self.sent.saturating_sub(self.acked) <= 1
    }
}

#[derive(Default)]
pub struct Pacer {
    views: Vec<View>,
    /// The screen changed since the last frame.
    dirty: bool,
    /// The next frame is full for every view (a resize, a new theme).
    force_full: bool,
    last_frame: Option<Instant>,
    /// When the current hold began.
    hold_since: Option<Instant>,
}

impl Pacer {
    /// The screen changed.
    pub fn changed(&mut self) {
        self.dirty = true;
    }

    /// The screen changed since the last frame.
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// The screen changed all over: the next frame is full.
    pub fn changed_all(&mut self) {
        self.dirty = true;
        self.force_full = true;
    }

    /// Send frames to `sink` as view `id`, starting with a full frame. A
    /// view replacing one by the same id carries on its seq; frames sent to
    /// the old one will never be acked.
    pub fn add_view(&mut self, id: ViewId, sink: FrameSink) {
        let sent = self.view(id).map_or(0, |v| v.sent);
        self.remove_view(id);
        self.views.push(View {
            id,
            sink,
            sent,
            acked: sent,
            current: false,
        });
    }

    pub fn remove_view(&mut self, id: ViewId) {
        self.views.retain(|v| v.id != id);
    }

    /// View `id` painted every frame up to `seq`.
    pub fn ack(&mut self, id: ViewId, seq: u64) {
        if let Some(v) = self.view(id) {
            v.acked = v.acked.max(seq.min(v.sent));
        }
    }

    /// View `id` missed a frame and drops partial ones until a full one:
    /// frames in flight will never be acked, and waiting on them would
    /// stall it for good.
    pub fn request_full(&mut self, id: ViewId) {
        if let Some(v) = self.view(id) {
            v.current = false;
            v.acked = v.sent;
        }
    }

    /// Do what is due at `now`: end a hold that lasted too long, send a
    /// frame. Returns when to come back if nothing else happens first.
    pub fn tick(&mut self, emu: &mut Emulator, now: Instant) -> Option<Instant> {
        if emu.held() {
            let since = *self.hold_since.get_or_insert(now);
            if now >= since + HOLD_TIMEOUT {
                emu.end_hold();
                self.hold_since = None;
                self.dirty = true;
            }
        } else {
            self.hold_since = None;
        }
        let hold_end = self.hold_since.map(|t| t + HOLD_TIMEOUT);
        let frame_due = self.send(emu, now);
        [hold_end, frame_due].into_iter().flatten().min()
    }

    /// Send what changed now to the views keeping up, whether or not a
    /// frame is due or the screen is held: the last frame before the
    /// session ends.
    pub fn flush(&mut self, emu: &mut Emulator, now: Instant) {
        if !self.any_ready() {
            return;
        }
        if let Some(f) = emu.take_captured() {
            self.emit(f, now);
        }
        if self.any_ready() {
            self.build(emu, now);
        }
    }

    /// Send a frame if one is due. Returns when one will be, if waiting
    /// on the clock.
    fn send(&mut self, emu: &mut Emulator, now: Instant) -> Option<Instant> {
        if !self.any_ready() {
            return None;
        }
        // The frame captured at the start of a hold goes first.
        if let Some(f) = emu.take_captured() {
            self.emit(f, now);
        }
        let full = self.needs_full();
        if !(self.dirty || full) || emu.held() {
            return None;
        }
        if let Some(t) = self.last_frame {
            let due = t + FRAME_INTERVAL;
            if now < due && !full {
                return Some(due);
            }
        }
        self.build(emu, now);
        None
    }

    fn any_ready(&self) -> bool {
        self.views.iter().any(View::ready)
    }

    /// A ready view missed a frame, or every view needs a full one.
    fn needs_full(&self) -> bool {
        self.force_full || self.views.iter().any(|v| v.ready() && !v.current)
    }

    fn build(&mut self, emu: &mut Emulator, now: Instant) {
        let full = self.needs_full();
        if let Ok(Some(f)) = emu.build_frame(full) {
            self.emit(f, now);
        }
        self.dirty = false;
        self.force_full = false;
    }

    /// `f` to every view that can take it; the rest miss it.
    fn emit(&mut self, mut f: Frame, now: Instant) {
        self.last_frame = Some(now);
        for v in &mut self.views {
            if v.ready() && (v.current || f.full) {
                v.sent += 1;
                f.seq = v.sent;
                (v.sink)(f.clone());
                v.current = true;
            } else {
                v.current = false;
            }
        }
    }

    fn view(&mut self, id: ViewId) -> Option<&mut View> {
        self.views.iter_mut().find(|v| v.id == id)
    }
}
