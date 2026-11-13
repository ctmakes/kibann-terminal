//! `kibann pane diff P --view`: a full-screen viewer of a pane's
//! uncommitted changes, run in a split beside it. A file list on top, the
//! selected file's patch below; it follows changes while open.
//!
//! Drawing is plain text with SGR colors, built by [`View::render`] (no
//! terminal needed, so it's tested directly). The terminal side (raw mode,
//! keys, the daemon connection) is Unix-only, like the CLI's socket.

use proto::git::{Diff, FileDiff};

/// What a key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    PageDown,
    PageUp,
    Top,
    Bottom,
    Quit,
}

/// Keys in a chunk of input. Unknown bytes are skipped.
pub fn keys(bytes: &[u8]) -> Vec<Key> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &bytes[i..];
        let (key, used) = match rest {
            [0x1b, b'[' | b'O', b'A', ..] => (Some(Key::Up), 3),
            [0x1b, b'[' | b'O', b'B', ..] => (Some(Key::Down), 3),
            [0x1b, b'[', b'5', b'~', ..] => (Some(Key::PageUp), 4),
            [0x1b, b'[', b'6', b'~', ..] => (Some(Key::PageDown), 4),
            [0x1b, b'[' | b'O', b'H', ..] => (Some(Key::Top), 3),
            [0x1b, b'[' | b'O', b'F', ..] => (Some(Key::Bottom), 3),
            // Any other sequence: skip to its final byte.
            [0x1b, b'[', ..] => {
                let end = rest[2..]
                    .iter()
                    .position(|b| (0x40..=0x7e).contains(b))
                    .map_or(rest.len(), |p| p + 3);
                (None, end)
            }
            // A lone Escape.
            [0x1b] => (Some(Key::Quit), 1),
            [0x1b, ..] => (None, 2),
            [b'k', ..] => (Some(Key::Up), 1),
            [b'j', ..] => (Some(Key::Down), 1),
            [b' ' | b'f', ..] => (Some(Key::PageDown), 1),
            [b'b', ..] => (Some(Key::PageUp), 1),
            [b'g', ..] => (Some(Key::Top), 1),
            [b'G', ..] => (Some(Key::Bottom), 1),
            [b'q' | 0x03 | 0x04, ..] => (Some(Key::Quit), 1),
            _ => (None, 1),
        };
        out.extend(key);
        i += used;
    }
    out
}

/// The viewer's state.
#[derive(Debug, Default)]
pub struct View {
    pub diff: Option<Diff>,
    /// Shown instead of the diff: the pane closed, or isn't in a repo.
    pub note: Option<String>,
    pub selected: usize,
    /// First patch line shown.
    pub scroll: usize,
    /// Where the pane is, for the title.
    pub title: String,
}

const RESET: &str = "\x1b[0m";

impl View {
    /// Take a fresh diff, keeping the selected file (by path) and, if it's
    /// still there, the scroll.
    pub fn set_diff(&mut self, diff: Diff) {
        let path = self.file().map(|f| f.path.clone());
        let at = path.and_then(|p| diff.files.iter().position(|f| f.path == p));
        match at {
            Some(i) => self.selected = i,
            None => {
                self.selected = self.selected.min(diff.files.len().saturating_sub(1));
                self.scroll = 0;
            }
        }
        self.diff = Some(diff);
        self.note = None;
    }

    fn files(&self) -> &[FileDiff] {
        self.diff.as_ref().map_or(&[], |d| &d.files)
    }

    fn file(&self) -> Option<&FileDiff> {
        self.files().get(self.selected)
    }

    /// Lines of the selected file's body.
    fn body(&self) -> Vec<String> {
        let Some(f) = self.file() else {
            return Vec::new();
        };
        let mut lines: Vec<String> = match &f.patch {
            Some(p) => p.lines().map(clean).collect(),
            None if f.binary => vec!["Binary file, not shown.".into()],
            None => vec!["Too large to show.".into()],
        };
        if f.truncated {
            lines.push("[cut at the size limit]".into());
        }
        lines
    }

    /// Rows for the file list in a screen of `rows`.
    fn list_rows(&self, rows: usize) -> usize {
        let n = self.files().len();
        // Title, separator and footer take three rows; the list gets at
        // most a third of what's left, and the patch the rest.
        n.min((rows.saturating_sub(3) / 3).max(1))
    }

    /// Rows for the patch in a screen of `rows`.
    fn patch_rows(&self, rows: usize) -> usize {
        rows.saturating_sub(3 + self.list_rows(rows)).max(1)
    }

    pub fn key(&mut self, key: Key, rows: usize) {
        let n = self.files().len();
        let page = self.patch_rows(rows).saturating_sub(1).max(1);
        let last = self.body().len().saturating_sub(self.patch_rows(rows));
        match key {
            Key::Up if self.selected > 0 => {
                self.selected -= 1;
                self.scroll = 0;
            }
            Key::Down if self.selected + 1 < n => {
                self.selected += 1;
                self.scroll = 0;
            }
            Key::PageDown => self.scroll = (self.scroll + page).min(last),
            Key::PageUp => self.scroll = self.scroll.saturating_sub(page),
            Key::Top => self.scroll = 0,
            Key::Bottom => self.scroll = last,
            _ => {}
        }
    }

    /// The whole screen: home, then every row (cleared to its end).
    pub fn render(&self, rows: usize, cols: usize) -> String {
        let rows = rows.max(4);
        let cols = cols.max(10);
        let mut out = String::from("\x1b[H");
        let mut line = |text: &str, style: &str| {
            out.push_str(style);
            out.push_str(&fit(text, cols));
            out.push_str(RESET);
            out.push_str("\x1b[K\r\n");
        };

        let files = self.files();
        let count = match files.len() {
            1 => "1 file".to_owned(),
            n => format!("{n} files"),
        };
        line(&format!(" {} · {count}", self.title), "\x1b[1m");

        if let Some(note) = &self.note {
            line("", "");
            line(&format!(" {note}"), "\x1b[2m");
            for _ in 3..rows - 1 {
                line("", "");
            }
            footer(&mut out, cols);
            return out;
        }
        if files.is_empty() {
            line("", "");
            let text = if self.diff.is_some() {
                " No uncommitted changes."
            } else {
                " Loading…"
            };
            line(text, "\x1b[2m");
            for _ in 3..rows - 1 {
                line("", "");
            }
            footer(&mut out, cols);
            return out;
        }

        // The list, scrolled to keep the selection in view.
        let list = self.list_rows(rows);
        let first = self.selected.saturating_sub(list - 1);
        for (i, f) in files.iter().enumerate().skip(first).take(list) {
            let counts = match (f.added, f.removed) {
                _ if f.binary => "bin".to_owned(),
                (Some(a), Some(r)) => format!("+{a} -{r}"),
                _ => String::new(),
            };
            let name = match &f.old_path {
                Some(old) => format!("{} → {}", clean(old), clean(&f.path)),
                None => clean(&f.path),
            };
            let marker = if i == self.selected { "›" } else { " " };
            let left = format!("{marker} {} {name}", f.status);
            let style = if i == self.selected { "\x1b[7m" } else { "" };
            line(&spread(&left, &counts, cols), style);
        }
        let more = if self.diff.as_ref().is_some_and(|d| d.truncated) {
            " more changes not shown (size limit) "
        } else {
            ""
        };
        line(&format!("{more:─<cols$}"), "\x1b[2m");

        let body = self.body();
        let height = self.patch_rows(rows);
        let scroll = self.scroll.min(body.len().saturating_sub(height));
        for i in 0..height {
            match body.get(scroll + i) {
                Some(l) => line(l, patch_style(l)),
                None => line("", ""),
            }
        }
        footer(&mut out, cols);
        out
    }
}

fn footer(out: &mut String, cols: usize) {
    out.push_str("\x1b[2m");
    out.push_str(&fit(" ↑↓ file   space/b page   q close", cols));
    out.push_str(RESET);
    out.push_str("\x1b[K");
}

fn patch_style(line: &str) -> &'static str {
    if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff ") {
        "\x1b[1m"
    } else if line.starts_with('+') {
        "\x1b[32m"
    } else if line.starts_with('-') {
        "\x1b[31m"
    } else if line.starts_with("@@") {
        "\x1b[36m"
    } else if line.starts_with(' ') || line.is_empty() {
        ""
    } else {
        "\x1b[2m"
    }
}

/// Text from the repo made safe to print: tabs to spaces, other control
/// characters shown as `?`, so file content can't move the cursor or
/// change the terminal.
fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\t' => {
                let col = out.chars().count();
                out.extend(std::iter::repeat_n(' ', 4 - col % 4));
            }
            c if c.is_control() => out.push('?'),
            c => out.push(c),
        }
    }
    out
}

/// `text` cut to `cols` characters.
fn fit(text: &str, cols: usize) -> String {
    text.chars().take(cols).collect()
}

/// `left` and `right` on one line of `cols`, `right` flush right.
fn spread(left: &str, right: &str, cols: usize) -> String {
    let (l, r) = (left.chars().count(), right.chars().count());
    if l + r + 2 > cols {
        return fit(left, cols);
    }
    format!("{left}{}{right} ", " ".repeat(cols - l - r - 1))
}

#[cfg(unix)]
pub use term::run;

#[cfg(unix)]
mod term {
    use std::io::{self, Read, Write};
    use std::sync::mpsc;
    use std::time::Duration;

    use proto::git::PaneDir;
    use proto::layout::PaneId;
    use proto::wire::{ClientMsg, Reply};

    use super::{Key, View, keys};
    use crate::client::{Client, ClientEvent};

    /// How often to re-fetch even without a status change: editing a file
    /// that's already changed doesn't change the count.
    const REFRESH: Duration = Duration::from_secs(3);

    enum Ev {
        Input(Vec<u8>),
        Changed,
        Closed,
    }

    /// The terminal in raw mode on the alternate screen, until dropped.
    struct Screen {
        saved: libc::termios,
    }

    impl Screen {
        fn enter() -> io::Result<Self> {
            // SAFETY: tcgetattr/tcsetattr on stdin with a zeroed termios
            // that tcgetattr fills.
            unsafe {
                let mut t: libc::termios = std::mem::zeroed();
                if libc::tcgetattr(0, &mut t) != 0 {
                    return Err(io::Error::last_os_error());
                }
                let saved = t;
                libc::cfmakeraw(&mut t);
                if libc::tcsetattr(0, libc::TCSANOW, &t) != 0 {
                    return Err(io::Error::last_os_error());
                }
                // Alternate screen, cursor hidden, no autowrap.
                print!("\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[2J");
                io::stdout().flush()?;
                Ok(Self { saved })
            }
        }

        fn size() -> (usize, usize) {
            // SAFETY: TIOCGWINSZ fills a winsize.
            unsafe {
                let mut w: libc::winsize = std::mem::zeroed();
                if libc::ioctl(1, libc::TIOCGWINSZ, &mut w) == 0 && w.ws_row > 0 {
                    (w.ws_row as usize, w.ws_col as usize)
                } else {
                    (24, 80)
                }
            }
        }
    }

    impl Drop for Screen {
        fn drop(&mut self) {
            print!("\x1b[?7h\x1b[?25h\x1b[?1049l");
            let _ = io::stdout().flush();
            // SAFETY: restoring the attributes tcgetattr gave.
            unsafe {
                libc::tcsetattr(0, libc::TCSANOW, &self.saved);
            }
        }
    }

    /// Show `pane`'s changes until q. `connect` gives a client whose
    /// events go to the callback.
    pub fn run(
        pane: PaneId,
        connect: impl FnOnce(Box<dyn FnMut(ClientEvent<'_>) + Send>) -> io::Result<Client>,
    ) -> io::Result<()> {
        let (tx, rx) = mpsc::channel();
        let events = tx.clone();
        let mut last: Option<Option<PaneDir>> = None;
        let client = connect(Box::new(move |e| match e {
            ClientEvent::Git(mut panes) => {
                let mine = panes.remove(&pane);
                if last.as_ref() != Some(&mine) {
                    last = Some(mine);
                    let _ = events.send(Ev::Changed);
                }
            }
            ClientEvent::Layout(l) if l.tab_of(pane).is_none() => {
                let _ = events.send(Ev::Closed);
            }
            ClientEvent::Disconnected => {
                let _ = events.send(Ev::Closed);
            }
            _ => {}
        }))?;
        client.watch()?;

        let input = tx;
        std::thread::spawn(move || {
            let mut buf = [0u8; 256];
            let mut stdin = io::stdin();
            while let Ok(n) = stdin.read(&mut buf) {
                if n == 0 || input.send(Ev::Input(buf[..n].to_vec())).is_err() {
                    break;
                }
            }
        });

        let screen = Screen::enter()?;
        let mut view = View {
            title: format!("p{pane} changes"),
            ..View::default()
        };
        let mut size = Screen::size();
        let mut fetch = true;
        let mut out = io::stdout();
        loop {
            if fetch && view.note.is_none() {
                match client.request(|req| ClientMsg::Diff { req, pane }) {
                    Ok(Reply::Diff { diff }) => view.set_diff(diff),
                    Ok(Reply::Error { message }) => view.note = Some(message),
                    Ok(_) => {}
                    Err(e) => view.note = Some(format!("lost the daemon: {e}")),
                }
                fetch = false;
            }
            out.write_all(view.render(size.0, size.1).as_bytes())?;
            out.flush()?;
            match rx.recv_timeout(REFRESH) {
                Ok(Ev::Input(bytes)) => {
                    for k in keys(&bytes) {
                        if k == Key::Quit {
                            drop(screen);
                            return Ok(());
                        }
                        view.key(k, size.0);
                    }
                }
                Ok(Ev::Changed) => fetch = true,
                Ok(Ev::Closed) => view.note = Some("The pane closed.".into()),
                Err(mpsc::RecvTimeoutError::Timeout) => fetch = true,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            let now = Screen::size();
            if now != size {
                size = now;
                out.write_all(b"\x1b[2J")?;
            }
        }
        drop(screen);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, patch: &str) -> FileDiff {
        FileDiff {
            path: path.into(),
            old_path: None,
            status: "M".into(),
            added: Some(1),
            removed: Some(1),
            binary: false,
            patch: Some(patch.into()),
            truncated: false,
        }
    }

    fn plain(s: &str) -> String {
        let re = regex::Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]").unwrap();
        re.replace_all(s, "").into_owned()
    }

    fn view() -> View {
        let mut v = View {
            title: "p1 changes".into(),
            ..View::default()
        };
        let long: String = (0..50).map(|i| format!("+line {i}\n")).collect();
        v.set_diff(Diff {
            files: vec![
                file("a.txt", "diff --git a/a.txt b/a.txt\n-old\n+new\n"),
                file("b.txt", &long),
            ],
            truncated: false,
        });
        v
    }

    #[test]
    fn keys_from_bytes() {
        assert_eq!(
            keys(b"jk\x1b[A\x1b[B \x1b[5~b\x1b[1;5Cq"),
            [
                Key::Down,
                Key::Up,
                Key::Up,
                Key::Down,
                Key::PageDown,
                Key::PageUp,
                Key::PageUp,
                Key::Quit
            ]
        );
        assert_eq!(keys(b"\x1b"), [Key::Quit]);
        assert_eq!(keys(b"\x03"), [Key::Quit]);
    }

    #[test]
    fn draws_the_list_and_the_patch() {
        let v = view();
        let screen = plain(&v.render(12, 40));
        let rows: Vec<&str> = screen.split("\r\n").collect();
        assert_eq!(rows.len(), 12, "{screen}");
        assert!(rows[0].contains("p1 changes · 2 files"));
        assert!(rows[1].starts_with("› M a.txt") && rows[1].ends_with("+1 -1 "));
        assert!(rows[2].starts_with("  M b.txt"));
        assert!(rows[3].starts_with('─'));
        assert_eq!(rows[5], "-old");
        assert_eq!(rows[6], "+new");
        assert!(rows[11].contains("q close"));
        // Colored: added lines green, removed red.
        let raw = v.render(12, 40);
        assert!(raw.contains("\x1b[32m+new"));
        assert!(raw.contains("\x1b[31m-old"));
    }

    #[test]
    fn moving_and_paging() {
        let mut v = view();
        v.key(Key::Down, 12);
        assert_eq!(v.selected, 1);
        v.key(Key::Down, 12);
        assert_eq!(v.selected, 1, "stays on the last file");
        let rows = plain(&v.render(12, 40));
        assert!(rows.contains("+line 0"));
        v.key(Key::PageDown, 12);
        let rows = plain(&v.render(12, 40));
        assert!(!rows.contains("+line 0\r") && rows.contains("+line 6"));
        v.key(Key::Bottom, 12);
        assert!(plain(&v.render(12, 40)).contains("+line 49"));
        v.key(Key::Up, 12);
        assert_eq!((v.selected, v.scroll), (0, 0));
    }

    #[test]
    fn a_new_diff_keeps_the_selected_file() {
        let mut v = view();
        v.key(Key::Down, 12);
        v.key(Key::PageDown, 12);
        let scroll = v.scroll;
        let mut d = v.diff.clone().unwrap();
        d.files.insert(0, file("0.txt", "+x\n"));
        v.set_diff(d);
        assert_eq!(v.file().unwrap().path, "b.txt");
        assert_eq!(v.scroll, scroll);
        v.set_diff(Diff::default());
        assert_eq!(v.selected, 0);
        assert!(plain(&v.render(10, 40)).contains("No uncommitted changes."));
    }

    #[test]
    fn file_content_cannot_reach_the_terminal() {
        let mut v = View::default();
        v.set_diff(Diff {
            files: vec![file("x\x1b]0;evil\x07", "+\x1b[2J\tz\n")],
            truncated: false,
        });
        let raw = v.render(10, 40);
        assert!(!raw.contains("\x1b]") && !raw.contains("\x1b[2J"));
        assert!(plain(&raw).contains("+?[2J   z"));
    }
}
