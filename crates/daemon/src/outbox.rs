//! A connection's queue of encoded messages, drained by its writer thread.
//!
//! Sessions push frames from their own threads and must never block on a
//! slow client, so frames beyond [`MAX_FRAMES`] are dropped: the client sees
//! the gap in `seq` and asks for a full frame. A snapshot (the layout,
//! agent status, git) replaces one of its kind still waiting, so a stalled
//! client holds at most one of each. Other messages (replies) are always
//! kept.

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};

/// Frames queued at most. Ack-based pacing keeps two or so in flight per
/// session, so this only fills when the client stops reading.
pub const MAX_FRAMES: usize = 64;

/// A message that says all there is to know of its kind: a newer one
/// makes a waiting one useless.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Snapshot {
    Layout,
    Status,
    Git,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Frame,
    Kept,
    Latest(Snapshot),
}

#[derive(Default)]
struct Queue {
    items: VecDeque<(Vec<u8>, Kind)>,
    frames: usize,
    closed: bool,
    dropped: u64,
}

#[derive(Default)]
pub struct Outbox {
    q: Mutex<Queue>,
    cv: Condvar,
}

impl Outbox {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a frame, or drop it if too many are waiting.
    pub fn push_frame(&self, bytes: Vec<u8>) {
        let mut q = self.q.lock().unwrap();
        if q.closed {
            return;
        }
        if q.frames >= MAX_FRAMES {
            q.dropped += 1;
            return;
        }
        q.frames += 1;
        q.items.push_back((bytes, Kind::Frame));
        self.cv.notify_one();
    }

    /// Queue a message that must arrive.
    pub fn push(&self, bytes: Vec<u8>) {
        let mut q = self.q.lock().unwrap();
        if q.closed {
            return;
        }
        q.items.push_back((bytes, Kind::Kept));
        self.cv.notify_one();
    }

    /// Queue a snapshot, in place of one of its kind still waiting (which
    /// so arrives sooner, and newer).
    pub fn push_latest(&self, kind: Snapshot, bytes: Vec<u8>) {
        let mut q = self.q.lock().unwrap();
        if q.closed {
            return;
        }
        let kind = Kind::Latest(kind);
        match q.items.iter_mut().find(|(_, k)| *k == kind) {
            Some(waiting) => waiting.0 = bytes,
            None => {
                q.items.push_back((bytes, kind));
                self.cv.notify_one();
            }
        }
    }

    /// The next message, waiting for one. `None` once closed.
    pub fn pop(&self) -> Option<Vec<u8>> {
        let mut q = self.q.lock().unwrap();
        loop {
            if q.closed {
                return None;
            }
            if let Some((bytes, kind)) = q.items.pop_front() {
                if kind == Kind::Frame {
                    q.frames -= 1;
                }
                return Some(bytes);
            }
            q = self.cv.wait(q).unwrap();
        }
    }

    /// Stop: pending and future messages are discarded and `pop` returns `None`.
    pub fn close(&self) {
        let mut q = self.q.lock().unwrap();
        q.closed = true;
        q.items.clear();
        self.cv.notify_all();
    }

    /// Frames dropped because the client fell behind.
    pub fn dropped(&self) -> u64 {
        self.q.lock().unwrap().dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_drop_when_full_but_other_messages_stay() {
        let o = Outbox::new();
        for i in 0..MAX_FRAMES + 5 {
            o.push_frame(vec![i as u8]);
        }
        o.push(b"exit".to_vec());
        assert_eq!(o.dropped(), 5);
        for i in 0..MAX_FRAMES {
            assert_eq!(o.pop(), Some(vec![i as u8]));
        }
        assert_eq!(o.pop(), Some(b"exit".to_vec()));
        // Room again.
        o.push_frame(vec![1]);
        assert_eq!(o.pop(), Some(vec![1]));
        o.close();
        assert_eq!(o.pop(), None);
    }

    #[test]
    fn a_snapshot_replaces_a_waiting_one_of_its_kind() {
        let o = Outbox::new();
        o.push_latest(Snapshot::Layout, b"layout 1".to_vec());
        o.push(b"reply".to_vec());
        o.push_latest(Snapshot::Git, b"git 1".to_vec());
        o.push_latest(Snapshot::Layout, b"layout 2".to_vec());
        assert_eq!(o.pop(), Some(b"layout 2".to_vec()));
        assert_eq!(o.pop(), Some(b"reply".to_vec()));
        // Once sent, the next one queues again.
        o.push_latest(Snapshot::Layout, b"layout 3".to_vec());
        assert_eq!(o.pop(), Some(b"git 1".to_vec()));
        assert_eq!(o.pop(), Some(b"layout 3".to_vec()));
    }
}
