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

use proto::layout::{Layout, PaneId, Tree};
use serde::{Deserialize, Serialize};

/// How long the layout must stay the same before it's written.
pub const SAVE_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Saved {
    pub layout: Layout,
    /// Each pane's last directory, when known.
    pub dirs: BTreeMap<PaneId, String>,
}

impl Saved {
    pub fn panes(&self) -> usize {
        self.layout.tabs.iter().map(|t| t.tree.panes().len()).sum()
    }
}

/// Writes the newest layout offered, once it has been the same for
/// `delay`, on a thread of its own.
pub struct Saver {
    path: PathBuf,
    tx: Sender<String>,
    /// What was last offered, to skip offering it again.
    last: String,
}

impl Saver {
    pub fn start(path: PathBuf, delay: Duration) -> Self {
        let (tx, rx) = mpsc::channel::<String>();
        let to = path.clone();
        let _ = std::thread::Builder::new()
            .name("layout-saver".into())
            .spawn(move || {
                while let Ok(mut latest) = rx.recv() {
                    loop {
                        match rx.recv_timeout(delay) {
                            Ok(s) => latest = s,
                            Err(RecvTimeoutError::Timeout) => break,
                            // Ending (on purpose): a pending change is dropped.
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                    }
                    if let Err(e) = write(&to, &latest) {
                        eprintln!("kibannd: saving the layout to {}: {e}", to.display());
                    }
                }
            });
        let last = std::fs::read_to_string(&path).unwrap_or_default();
        Self { path, tx, last }
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
            let _ = self.tx.send(text);
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
    let text = std::fs::read_to_string(path).ok()?;
    let s: Saved = serde_json::from_str(&text).ok()?;
    (s.panes() > 0).then_some(s)
}

/// `tree` with each pane replaced as `f` says.
pub fn map_tree(tree: &Tree, f: &mut impl FnMut(PaneId) -> PaneId) -> Tree {
    match tree {
        Tree::Leaf { pane } => Tree::leaf(f(*pane)),
        Tree::Split { dir, ratio, a, b } => Tree::Split {
            dir: *dir,
            ratio: *ratio,
            a: Box::new(map_tree(a, f)),
            b: Box::new(map_tree(b, f)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::layout::{Dir, Tab};

    fn sample() -> Saved {
        let tree = Tree::Split {
            dir: Dir::Row,
            ratio: 0.3,
            a: Box::new(Tree::leaf(4)),
            b: Box::new(Tree::leaf(7)),
        };
        Saved {
            layout: Layout {
                tabs: vec![Tab {
                    id: 2,
                    tree,
                    focused: 7,
                    zoomed: None,
                }],
                active: Some(2),
            },
            dirs: BTreeMap::from([(4, "/tmp".to_owned())]),
        }
    }

    #[test]
    fn written_after_the_delay_and_read_back() {
        let dir = std::env::temp_dir().join(format!("kibann-saved-{}", std::process::id()));
        let path = dir.join("layout.json");
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Saver::start(path.clone(), Duration::from_millis(50));
        let mut empty = sample();
        empty.layout.tabs.clear();
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
    fn trees_map_to_new_panes() {
        let t = map_tree(&sample().layout.tabs[0].tree, &mut |p| p + 100);
        assert_eq!(t.panes(), [104, 107]);
    }
}
