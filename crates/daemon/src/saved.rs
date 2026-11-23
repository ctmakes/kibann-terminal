//! The layout kept on disk, so the tabs and splits come back (with fresh
//! shells in the same directories) after the sessions ended: a restart, a
//! log out, "Quit and End Sessions".
//!
//! Written a few seconds after the last change, so a log out, which ends
//! the shells one by one just before the daemon, doesn't save the layout
//! shrinking to nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

use proto::layout::{Layout, PaneId, Workspace};
use serde::{Deserialize, Serialize};

/// How long the layout must stay the same before it's written.
pub const SAVE_DELAY: Duration = Duration::from_secs(5);

/// What's kept: the window as a workspace (opened again by
/// [`ClientMsg::Restore`](proto::wire::ClientMsg::Restore)), and the theme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub workspace: Workspace,
    /// The sessions' colors, last set by a client: a daemon starting again
    /// uses them from the first pane on.
    #[serde(default)]
    pub theme: Option<proto::theme::Theme>,
}

/// The file as 0.7 wrote it. Read once, by the first daemon after the
/// update; remove in 0.9.
#[derive(Deserialize)]
struct Before {
    layout: Layout,
    dirs: BTreeMap<PaneId, String>,
    #[serde(default)]
    theme: Option<proto::theme::Theme>,
}

/// The saved file, in either shape.
fn read(path: &Path) -> Option<Saved> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Saved>(&text).ok().or_else(|| {
        let b: Before = serde_json::from_str(&text).ok()?;
        Some(Saved {
            workspace: Workspace::of(&b.layout, |p| b.dirs.get(&p).cloned()),
            theme: b.theme,
        })
    })
}

/// Writes the newest layout offered, once it has been the same for
/// `delay`, on a thread of its own.
pub struct Saver {
    path: PathBuf,
    tx: Sender<Msg>,
    /// What was last offered, to skip offering it again.
    last: String,
}

enum Msg {
    Offer(String),
    /// Write what's pending now, then answer.
    Flush(Sender<()>),
}

impl Saver {
    pub fn start(path: PathBuf, delay: Duration) -> Self {
        let (tx, rx) = mpsc::channel::<Msg>();
        let to = path.clone();
        let write_to = move |text: &str| {
            if let Err(e) = write(&to, text) {
                eprintln!("kibannd: saving the layout to {}: {e}", to.display());
            }
        };
        let _ = std::thread::Builder::new()
            .name("layout-saver".into())
            .spawn(move || {
                while let Ok(msg) = rx.recv() {
                    let mut latest = match msg {
                        Msg::Offer(s) => s,
                        Msg::Flush(done) => {
                            let _ = done.send(());
                            continue;
                        }
                    };
                    loop {
                        match rx.recv_timeout(delay) {
                            Ok(Msg::Offer(s)) => latest = s,
                            Ok(Msg::Flush(done)) => {
                                write_to(&latest);
                                let _ = done.send(());
                                break;
                            }
                            Err(RecvTimeoutError::Timeout) => {
                                write_to(&latest);
                                break;
                            }
                            // Ending without a flush: a pending change is
                            // dropped.
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                    }
                }
            });
        let last = std::fs::read_to_string(&path).unwrap_or_default();
        Self { path, tx, last }
    }

    /// Write the change waiting for its delay, if any, and return once it's
    /// on disk (or after `timeout`).
    pub fn flush(&self, timeout: Duration) {
        let (done, wait) = mpsc::channel();
        if self.tx.send(Msg::Flush(done)).is_ok() {
            let _ = wait.recv_timeout(timeout);
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Save `s` (soon) if it differs from what was saved or offered last.
    pub fn offer(&mut self, s: &Saved) {
        let Ok(text) = serde_json::to_string_pretty(s) else {
            return;
        };
        if text != self.last {
            self.last.clone_from(&text);
            let _ = self.tx.send(Msg::Offer(text));
        }
    }
}

/// Write through a temporary file, so a crash never leaves half a file.
fn write(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// The saved layout, if there is a readable one with panes in it.
pub fn load(path: &Path) -> Option<Saved> {
    read(path).filter(|s| s.workspace.panes() > 0)
}

/// The theme kept with the layout, if any (with or without panes).
pub fn load_theme(path: &Path) -> Option<proto::theme::Theme> {
    read(path)?.theme
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::layout::{Dir, Tab, Tree, WorkspaceTree};

    fn layout() -> Layout {
        let tree = Tree::Split {
            dir: Dir::Row,
            ratio: 0.3,
            a: Box::new(Tree::leaf(4)),
            b: Box::new(Tree::leaf(7)),
        };
        Layout {
            tabs: vec![Tab {
                id: 2,
                tree,
                focused: 7,
                zoomed: None,
                title: None,
            }],
            active: Some(2),
        }
    }

    fn sample() -> Saved {
        let dirs = BTreeMap::from([(4, "/tmp".to_owned())]);
        Saved {
            workspace: Workspace::of(&layout(), |p| dirs.get(&p).cloned()),
            theme: None,
        }
    }

    #[test]
    fn written_after_the_delay_and_read_back() {
        let dir = std::env::temp_dir().join(format!("kibann-saved-{}", std::process::id()));
        let path = dir.join("layout.json");
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Saver::start(path.clone(), Duration::from_millis(50));
        let mut empty = sample();
        empty.workspace.tabs.clear();
        // Changes in quick succession: only the last is written.
        s.offer(&empty);
        s.offer(&sample());
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(load(&path), Some(sample()));
        // An empty layout is saved, and reads as nothing to restore.
        s.offer(&empty);
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(load(&path), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_flush_writes_the_pending_change_now() {
        let dir = std::env::temp_dir().join(format!("kibann-flush-{}", std::process::id()));
        let path = dir.join("layout.json");
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Saver::start(path.clone(), Duration::from_secs(60));
        s.offer(&sample());
        s.flush(Duration::from_secs(5));
        assert_eq!(load(&path), Some(sample()));
        // Nothing pending: it returns at once.
        s.flush(Duration::from_secs(5));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_layout_becomes_a_workspace() {
        let w = sample().workspace;
        assert_eq!(w.panes(), 2);
        assert_eq!(w.tabs[0].focused, 1);
        let WorkspaceTree::Split { a, b, ratio, .. } = &w.tabs[0].tree else {
            panic!("a split");
        };
        assert_eq!(*ratio, 0.3);
        assert!(matches!(&**a, WorkspaceTree::Leaf { cwd, run: None } if cwd == "/tmp"));
        assert!(matches!(&**b, WorkspaceTree::Leaf { cwd, run: None } if cwd.is_empty()));
    }

    #[test]
    fn a_file_from_before_reads_as_a_workspace() {
        let dir = std::env::temp_dir().join(format!("kibann-before-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("layout.json");
        let before = serde_json::json!({
            "layout": layout(),
            "dirs": {"4": "/tmp"},
        });
        std::fs::write(&path, before.to_string()).unwrap();
        assert_eq!(load(&path), Some(sample()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
