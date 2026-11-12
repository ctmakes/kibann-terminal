//! Links under the pointer: hidden links (OSC 8), URLs and file paths found
//! in the text, and what each one opens.
//!
//! Text is handled as a list of cells (one grapheme cluster each, wide-char
//! tails left out), so a match maps straight back to grid positions.

use std::ops::Range;
use std::path::{Path, PathBuf};

use proto::{Link, LinkSpan, Resize};

use crate::vt::Terminal;
use crate::vt::error::Error;
use crate::vt::point::{Coordinate, Point};
use crate::vt::screen::CellWide;

/// How far a logical line is followed through wrapped rows, each way.
const MAX_WRAPPED_ROWS: u32 = 64;

/// URL schemes recognized in plain text.
const SCHEMES: &[&str] = &[
    "https://", "http://", "file://", "ftp://", "ssh://", "git://", "mailto:",
];

/// Something found in the text around a cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// A URL with one of [`SCHEMES`].
    Url(String),
    /// A path candidate; `path` is `text` without a `:line[:col]` suffix.
    Path { text: String, path: String },
}

fn first_char(cell: &str) -> char {
    cell.chars().next().unwrap_or(' ')
}

/// Ends a token: whitespace, empty cells, and quoting characters.
fn is_break(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>' | '|' | '\0')
}

fn text_of(cells: &[String], r: Range<usize>) -> String {
    cells[r].concat()
}

/// The whitespace-delimited token containing `at`.
fn token(cells: &[String], at: usize) -> Option<Range<usize>> {
    if at >= cells.len() || is_break(first_char(&cells[at])) {
        return None;
    }
    let mut start = at;
    while start > 0 && !is_break(first_char(&cells[start - 1])) {
        start -= 1;
    }
    let mut end = at + 1;
    while end < cells.len() && !is_break(first_char(&cells[end])) {
        end += 1;
    }
    Some(start..end)
}

/// Drop sentence punctuation and unbalanced brackets from the ends, as in
/// `(see https://example.com/a).`
fn trim(cells: &[String], mut r: Range<usize>) -> Range<usize> {
    let pairs = [('(', ')'), ('[', ']'), ('{', '}')];
    let count = |r: &Range<usize>, c: char| {
        cells[r.clone()]
            .iter()
            .filter(|s| first_char(s) == c)
            .count()
    };
    loop {
        if r.is_empty() {
            return r;
        }
        let last = first_char(&cells[r.end - 1]);
        let first = first_char(&cells[r.start]);
        if matches!(last, '.' | ',' | ';' | ':' | '!' | '?') {
            r.end -= 1;
            continue;
        }
        if let Some(&(open, _)) = pairs.iter().find(|p| p.1 == last)
            && count(&r, open) < count(&r, last)
        {
            r.end -= 1;
            continue;
        }
        if let Some(&(_, close)) = pairs.iter().find(|p| p.0 == first)
            && count(&r, close) < count(&r, first)
        {
            r.start += 1;
            continue;
        }
        return r;
    }
}

/// The URL around `at`: a known scheme, starting at or before `at` within
/// its token, not glued to a preceding letter (`xhttps://` isn't one).
fn url_at(cells: &[String], tok: Range<usize>, at: usize) -> Option<(Range<usize>, String)> {
    let text: Vec<char> = cells[tok.clone()].iter().map(|s| first_char(s)).collect();
    let mut best = None;
    for start in tok.start..=at {
        let i = start - tok.start;
        if i > 0 && text[i - 1].is_ascii_alphanumeric() {
            continue;
        }
        let rest: String = text[i..].iter().take(10).collect::<String>().to_lowercase();
        if let Some(s) = SCHEMES.iter().find(|s| rest.starts_with(**s)) {
            best = Some((start, s.len()));
        }
    }
    let (start, scheme_len) = best?;
    let r = trim(cells, start..tok.end);
    // A bare scheme isn't a link, and the match must still cover `at`.
    (r.len() > scheme_len && r.contains(&at)).then(|| {
        let s = text_of(cells, r.clone());
        (r, s)
    })
}

/// Split `path:12` or `path:12:3` into the path and its suffix.
pub fn strip_line_col(s: &str) -> &str {
    let mut out = s;
    for _ in 0..2 {
        match out.rsplit_once(':') {
            Some((head, n))
                if !head.is_empty() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) =>
            {
                out = head;
            }
            _ => break,
        }
    }
    out
}

/// The URL or path candidate around cell `at`.
pub fn find(cells: &[String], at: usize) -> Option<(Range<usize>, Found)> {
    let tok = token(cells, at)?;
    if let Some((r, url)) = url_at(cells, tok.clone(), at) {
        return Some((r, Found::Url(url)));
    }
    let r = trim(cells, tok);
    if !r.contains(&at) {
        return None;
    }
    let text = text_of(cells, r.clone());
    let path = strip_line_col(&text).to_owned();
    // Only dots ("." or "..") or a lone "~" would always exist.
    if path.chars().all(|c| c == '.') || path == "~" {
        return None;
    }
    Some((r, Found::Path { text, path }))
}

/// The absolute path a printed path names, if it exists. `~` is the home
/// directory; relative paths are taken from `cwd`.
pub fn resolve_path(path: &str, cwd: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    let p = if let Some(rest) = path.strip_prefix("~/") {
        home?.join(rest)
    } else if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        cwd?.join(path)
    };
    p.exists().then_some(p)
}

/// The home directory, from `HOME` (`USERPROFILE` on Windows).
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// A cell of a logical line: its text and where it sits on the screen.
struct LineCell {
    text: String,
    /// Screen row (scrollback included) and column.
    y: u32,
    x: u16,
    /// 2 for a wide character.
    width: u16,
    /// OSC 8 address, if the cell has one.
    uri: Option<String>,
}

fn screen(x: u16, y: u32) -> Point {
    Point::Screen(Coordinate { x, y })
}

fn read_uri(r: &crate::vt::grid_ref::GridRef<'_>) -> Option<String> {
    let mut buf = vec![0u8; 512];
    loop {
        match r.hyperlink_uri(&mut buf) {
            Ok(0) => return None,
            Ok(n) => return Some(String::from_utf8_lossy(&buf[..n]).into_owned()),
            Err(Error::OutOfSpace { required }) if required > buf.len() => buf.resize(required, 0),
            Err(_) => return None,
        }
    }
}

/// The logical line through screen row `y`: the row plus the rows it wraps
/// from and into.
fn logical_line(term: &Terminal<'_, '_>, cols: u16, y: u32, total: u32) -> Vec<LineCell> {
    let row_flags = |y: u32| {
        let row = term.grid_ref(screen(0, y)).ok()?.row().ok()?;
        Some((row.is_wrap_continuation().ok()?, row.is_wrapped().ok()?))
    };
    let mut top = y;
    while y - top < MAX_WRAPPED_ROWS && top > 0 && row_flags(top).is_some_and(|f| f.0) {
        top -= 1;
    }
    let mut bottom = y;
    while bottom - y < MAX_WRAPPED_ROWS
        && bottom + 1 < total
        && row_flags(bottom).is_some_and(|f| f.1)
    {
        bottom += 1;
    }

    let mut out = Vec::new();
    let mut chars = ['\0'; 16];
    for ry in top..=bottom {
        for x in 0..cols {
            let Ok(r) = term.grid_ref(screen(x, ry)) else {
                continue;
            };
            let Ok(cell) = r.cell() else { continue };
            let wide = cell.wide().unwrap_or(CellWide::Narrow);
            if matches!(wide, CellWide::SpacerTail | CellWide::SpacerHead) {
                continue;
            }
            let n = r.graphemes(&mut chars).unwrap_or(0);
            let text: String = if n == 0 {
                " ".into()
            } else {
                chars[..n.min(chars.len())].iter().collect()
            };
            let uri = if cell.has_hyperlink().unwrap_or(false) {
                read_uri(&r)
            } else {
                None
            };
            out.push(LineCell {
                text,
                y: ry,
                x,
                width: if wide == CellWide::Wide { 2 } else { 1 },
                uri,
            });
        }
    }
    out
}

/// The cells of `range` as spans per viewport row, leaving out rows outside
/// the viewport (`offset..offset + rows`).
fn spans(line: &[LineCell], range: Range<usize>, offset: u32, rows: u16) -> Vec<LinkSpan> {
    let mut out: Vec<LinkSpan> = Vec::new();
    let mut last_y = None;
    for c in &line[range] {
        let Some(vy) = c.y.checked_sub(offset).filter(|v| *v < u32::from(rows)) else {
            continue;
        };
        let vy = vy as u16;
        match out.last_mut() {
            Some(s) if last_y == Some(c.y) => s.len = c.x + c.width - s.x,
            _ => out.push(LinkSpan {
                y: vy,
                x: c.x,
                len: c.width,
            }),
        }
        last_y = Some(c.y);
    }
    out
}

/// The link at device-pixel position `x`, `y` in the viewport, if any.
/// Relative paths are taken from `cwd`.
pub fn at(
    term: &Terminal<'_, '_>,
    size: Resize,
    x: f32,
    y: f32,
    cwd: Option<&Path>,
) -> Option<Link> {
    let (cw, ch) = (f32::from(size.cell_w.max(1)), f32::from(size.cell_h.max(1)));
    if x < 0.0 || y < 0.0 {
        return None;
    }
    let (cx, cy) = ((x / cw) as u16, (y / ch) as u16);
    if cx >= size.cols || cy >= size.rows {
        return None;
    }
    let sb = term.scrollbar().ok()?;
    let offset = u32::try_from(sb.offset).ok()?;
    let total = u32::try_from(sb.total).ok()?;
    let line = logical_line(term, size.cols, offset + u32::from(cy), total);
    // The cell under the pointer; a wide character's tail counts as its head.
    let at = line
        .iter()
        .position(|c| c.y == offset + u32::from(cy) && (c.x..c.x + c.width).contains(&cx))?;

    if let Some(uri) = line[at].uri.clone() {
        let same = |c: &LineCell| c.uri.as_deref() == Some(uri.as_str());
        let start = line[..at]
            .iter()
            .rposition(|c| !same(c))
            .map_or(0, |i| i + 1);
        let end = line[at..]
            .iter()
            .position(|c| !same(c))
            .map_or(line.len(), |i| at + i);
        return Some(Link {
            target: uri.clone(),
            uri,
            spans: spans(&line, start..end, offset, size.rows),
        });
    }

    let cells: Vec<String> = line.iter().map(|c| c.text.clone()).collect();
    let (range, found) = find(&cells, at)?;
    let (uri, target) = match found {
        Found::Url(u) => (u.clone(), u),
        Found::Path { text, path } => {
            let p = resolve_path(&path, cwd, home_dir().as_deref())?;
            (text, p.to_string_lossy().into_owned())
        }
    };
    Some(Link {
        uri,
        target,
        spans: spans(&line, range, offset, size.rows),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(s: &str) -> Vec<String> {
        s.chars().map(String::from).collect()
    }

    /// The text found around the first occurrence of `needle` in `line`.
    fn found(line: &str, needle: &str) -> Option<Found> {
        let at = line.find(needle).expect("needle in line");
        let at = line[..at].chars().count();
        find(&cells(line), at).map(|(_, f)| f)
    }

    fn url(line: &str, needle: &str) -> Option<String> {
        match found(line, needle) {
            Some(Found::Url(u)) => Some(u),
            _ => None,
        }
    }

    #[test]
    fn urls_lose_sentence_punctuation_and_unbalanced_brackets() {
        assert_eq!(
            url("(see https://x.com/a).", "x.com").as_deref(),
            Some("https://x.com/a")
        );
        assert_eq!(
            url("go to https://x.com/search?q=a&b=c, then", "search").as_deref(),
            Some("https://x.com/search?q=a&b=c")
        );
        assert_eq!(
            url("https://en.wikipedia.org/wiki/Rust_(language)", "wiki").as_deref(),
            Some("https://en.wikipedia.org/wiki/Rust_(language)")
        );
        assert_eq!(
            url("<https://x.com/>", "x.com").as_deref(),
            Some("https://x.com/")
        );
        assert_eq!(
            url("url=\"http://a.b/c\"", "a.b").as_deref(),
            Some("http://a.b/c")
        );
        assert_eq!(url("mailto:a@b.c", "a@").as_deref(), Some("mailto:a@b.c"));
    }

    #[test]
    fn a_url_starts_at_its_scheme() {
        assert_eq!(
            url("link:https://x.com", "x.com").as_deref(),
            Some("https://x.com")
        );
        // Text before the scheme is not part of it.
        assert!(url("link:https://x.com", "link").is_none());
        assert!(url("xhttps://x.com", "x.com").is_none());
        assert!(url("https://", "https").is_none());
    }

    #[test]
    fn paths_keep_line_numbers_for_display_only() {
        assert_eq!(
            found("error at src/main.ts:12:3: bad", "main"),
            Some(Found::Path {
                text: "src/main.ts:12:3".into(),
                path: "src/main.ts".into()
            })
        );
        assert_eq!(strip_line_col("a.rs:7"), "a.rs");
        assert_eq!(strip_line_col("a.rs"), "a.rs");
        assert_eq!(strip_line_col("C:"), "C:");
        assert!(found("cd ..", "..").is_none());
        assert!(found("a  b", " ").is_none());
    }

    #[test]
    fn wide_characters_are_one_cell_each() {
        // Cells as the reader builds them: tails are dropped.
        let line: Vec<String> = ["界", " ", "h", "t", "t", "p", ":", "/", "/", "a", ".", "b"]
            .map(String::from)
            .to_vec();
        let (r, f) = find(&line, 10).unwrap();
        assert_eq!(r, 2..12);
        assert_eq!(f, Found::Url("http://a.b".into()));
    }

    #[test]
    fn paths_resolve_against_the_pane_directory() {
        let dir = std::env::temp_dir().join(format!("kibann-links-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.ts"), "").unwrap();
        let cwd = Some(dir.as_path());
        assert_eq!(
            resolve_path("src/main.ts", cwd, None),
            Some(dir.join("src/main.ts"))
        );
        assert_eq!(resolve_path("src", cwd, None), Some(dir.join("src")));
        assert_eq!(resolve_path("missing.ts", cwd, None), None);
        assert_eq!(resolve_path("~/src", None, cwd), Some(dir.join("src")));
        assert_eq!(resolve_path("src", None, None), None);
        let abs = dir.join("src/main.ts");
        assert_eq!(
            resolve_path(abs.to_str().unwrap(), None, None),
            Some(abs.clone())
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
