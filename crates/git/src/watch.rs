//! Keeps each checkout's [`GitInfo`] current from filesystem events: no
//! polling, so an idle workspace costs nothing.
//!
//! - One status per checkout, however many panes are in it
//!   ([`Watcher::add`] counts them).
//! - Events go through a [`Filter`]: ignored files (a build's output) and
//!   git internals like objects and logs start no run.
//! - The first event schedules a run [`DEBOUNCE`] later; events during a
//!   run schedule one more after it; while a checkout keeps changing it runs
//!   at most once per `max(1 s, 4 × last run)`, so a slow repo backs off.
//! - Two workers run `git status`, never more, and a run past
//!   [`RUN_TIMEOUT`] is killed.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};
use proto::git::GitInfo;

use crate::{Checkout, Filter};

/// How long after the first change a run starts, so a burst of writes (a
/// checkout, a save) costs one run.
pub const DEBOUNCE: Duration = Duration::from_millis(150);
/// Shortest gap between runs while changes keep coming.
const MIN_GAP: Duration = Duration::from_secs(1);
pub const RUN_TIMEOUT: Duration = Duration::from_secs(10);
const WORKERS: usize = 2;

/// Told a checkout's info changed: its root, and the info (`None` when git
/// status failed, as when the repository was deleted).
pub type OnChange = Box<dyn Fn(&Path, Option<&GitInfo>) + Send + Sync>;

struct Entry {
    checkout: Checkout,
    filter: Filter,
    /// Panes in it.
    refs: usize,
    /// Paths watched for it.
    watched: Vec<PathBuf>,
    due: Option<Instant>,
    running: bool,
    /// Changed during the run: run again after it.
    again: bool,
    started: Option<Instant>,
    took: Duration,
    info: Option<GitInfo>,
}

impl Entry {
    fn schedule(&mut self, now: Instant) {
        if self.running {
            self.again = true;
            return;
        }
        if self.due.is_none() {
            let gap = MIN_GAP.max(self.took * 4);
            let soonest = self.started.map_or(now, |s| s + gap);
            self.due = Some((now + DEBOUNCE).max(soonest));
        }
    }
}

#[derive(Default)]
struct State {
    checkouts: HashMap<PathBuf, Entry>,
    /// How many checkouts watch each path.
    watches: HashMap<PathBuf, usize>,
    global_excludes: Option<Option<PathBuf>>,
    stop: bool,
}

struct Shared {
    state: Mutex<State>,
    /// Kept apart from `state`, and taken before it: notify calls back on
    /// its own thread, which takes `state`, and with inotify `watch` and
    /// `unwatch` wait for that thread. Calling them under `state` deadlocks.
    fs: Mutex<Option<RecommendedWatcher>>,
    wake: Condvar,
    on_change: OnChange,
    runs: AtomicU64,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn fs(&self) -> MutexGuard<'_, Option<RecommendedWatcher>> {
        self.fs.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub struct Watcher {
    shared: Arc<Shared>,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.shared.lock().stop = true;
        self.shared.wake.notify_all();
        *self.shared.fs() = None;
    }
}

impl Watcher {
    pub fn new(on_change: impl Fn(&Path, Option<&GitInfo>) + Send + Sync + 'static) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::default(),
            fs: Mutex::default(),
            wake: Condvar::new(),
            on_change: Box::new(on_change),
            runs: AtomicU64::new(0),
        });
        let weak = Arc::downgrade(&shared);
        let fs = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let (Some(shared), Ok(event)) = (weak.upgrade(), res) {
                on_event(&shared, &event);
            }
        })
        .ok();
        *shared.fs() = fs;
        for i in 0..WORKERS {
            let s = shared.clone();
            let _ = thread::Builder::new()
                .name(format!("git-status-{i}"))
                .spawn(move || worker(&s));
        }
        Watcher { shared }
    }

    /// A pane is in `checkout`: watch it (the first pane starts a run at
    /// once). Returns its info if already known.
    pub fn add(&self, checkout: &Checkout) -> Option<GitInfo> {
        let mut fs = self.shared.fs();
        let mut st = self.shared.lock();
        if let Some(e) = st.checkouts.get_mut(&checkout.root) {
            e.refs += 1;
            return e.info.clone();
        }
        let global = st
            .global_excludes
            .get_or_insert_with(|| crate::global_excludes(&checkout.root))
            .clone();
        let mut watched = vec![checkout.root.clone()];
        for d in [&checkout.git_dir, &checkout.common_dir] {
            if !d.starts_with(&checkout.root) && !watched.contains(d) {
                watched.push(d.clone());
            }
        }
        let mut new = Vec::new();
        for p in &watched {
            let n = st.watches.entry(p.clone()).or_default();
            *n += 1;
            if *n == 1 {
                new.push(p.clone());
            }
        }
        st.checkouts.insert(
            checkout.root.clone(),
            Entry {
                checkout: checkout.clone(),
                filter: Filter::new(checkout, global.as_deref()),
                refs: 1,
                watched,
                due: Some(Instant::now()),
                running: false,
                again: false,
                started: None,
                took: Duration::ZERO,
                info: None,
            },
        );
        drop(st);
        if let Some(fs) = fs.as_mut() {
            for p in &new {
                let _ = fs.watch(p, RecursiveMode::Recursive);
            }
        }
        self.shared.wake.notify_all();
        None
    }

    /// A pane left the checkout at `root`; the last one stops watching it.
    pub fn remove(&self, root: &Path) {
        let mut fs = self.shared.fs();
        let mut st = self.shared.lock();
        let Some(e) = st.checkouts.get_mut(root) else {
            return;
        };
        e.refs -= 1;
        if e.refs > 0 {
            return;
        }
        let e = st.checkouts.remove(root).expect("present");
        let mut gone = Vec::new();
        for p in e.watched {
            let n = st.watches.get_mut(&p).expect("counted");
            *n -= 1;
            if *n == 0 {
                st.watches.remove(&p);
                gone.push(p);
            }
        }
        drop(st);
        if let Some(fs) = fs.as_mut() {
            for p in &gone {
                let _ = fs.unwatch(p);
            }
        }
    }

    /// Check `root` again soon, events or not (a shell prompt came back,
    /// an agent finished): events can be missed.
    pub fn refresh(&self, root: &Path) {
        let mut st = self.shared.lock();
        if let Some(e) = st.checkouts.get_mut(root) {
            e.schedule(Instant::now());
            drop(st);
            self.shared.wake.notify_all();
        }
    }

    /// The last info for the checkout at `root`.
    pub fn info(&self, root: &Path) -> Option<GitInfo> {
        self.shared.lock().checkouts.get(root)?.info.clone()
    }

    /// How many `git status` runs so far (for tests of the cost).
    pub fn runs(&self) -> u64 {
        self.shared.runs.load(Ordering::Relaxed)
    }

    /// Checkouts watched now.
    pub fn watching(&self) -> usize {
        self.shared.lock().checkouts.len()
    }
}

fn on_event(shared: &Shared, event: &notify::Event) {
    let now = Instant::now();
    let mut st = shared.lock();
    let mut any = false;
    for e in st.checkouts.values_mut() {
        if event.need_rescan() {
            e.schedule(now);
            any = true;
            continue;
        }
        for p in &event.paths {
            if e.filter.is_ignore_file(p) {
                // `.gitignore` changed: what counts changed with it.
                let global = crate::global_excludes(&e.checkout.root);
                e.filter = Filter::new(&e.checkout, global.as_deref());
            }
            if e.filter.matters(p) == Some(true) {
                e.schedule(now);
                any = true;
                break;
            }
        }
    }
    drop(st);
    if any {
        shared.wake.notify_all();
    }
}

fn worker(shared: &Shared) {
    let mut st = shared.lock();
    loop {
        if st.stop {
            return;
        }
        let now = Instant::now();
        let ready = st
            .checkouts
            .iter_mut()
            .filter(|(_, e)| !e.running && e.due.is_some_and(|d| d <= now))
            .min_by_key(|(_, e)| e.due)
            .map(|(root, e)| {
                e.running = true;
                e.due = None;
                e.started = Some(now);
                (root.clone(), e.checkout.clone())
            });
        let Some((root, checkout)) = ready else {
            let next = st
                .checkouts
                .values()
                .filter(|e| !e.running)
                .filter_map(|e| e.due)
                .min();
            st = match next {
                Some(t) => {
                    let wait = t.saturating_duration_since(now);
                    match shared.wake.wait_timeout(st, wait) {
                        Ok((g, _)) => g,
                        Err(e) => e.into_inner().0,
                    }
                }
                None => shared.wake.wait(st).unwrap_or_else(|e| e.into_inner()),
            };
            continue;
        };
        drop(st);

        shared.runs.fetch_add(1, Ordering::Relaxed);
        let info = run(&checkout);
        let took = now.elapsed();

        st = shared.lock();
        let Some(e) = st.checkouts.get_mut(&root) else {
            continue; // nobody is in it any more
        };
        e.running = false;
        e.took = took;
        if e.again {
            e.again = false;
            e.schedule(Instant::now());
            shared.wake.notify_all();
        }
        if e.info != info {
            e.info = info.clone();
            drop(st);
            (shared.on_change)(&root, info.as_ref());
            st = shared.lock();
        }
    }
}

/// [`Checkout::info`], killed after [`RUN_TIMEOUT`].
fn run(c: &Checkout) -> Option<GitInfo> {
    let mut cmd = crate::status_command(&c.root);
    cmd.stdout(Stdio::piped());
    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    // Read on another thread: a big status would fill the pipe and stall.
    let reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + RUN_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let out = reader.join().ok()?;
    status.filter(|s| s.success())?;
    Some(c.info_from(&crate::parse_status(&out)))
}
