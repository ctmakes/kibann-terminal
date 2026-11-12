//! The committed recordings replay to the committed frame files, and the
//! replays show what the programs drew.
//!
//! `UPDATE_RECORDINGS=1 cargo test -p term --test recordings` rewrites the
//! frame files (`npm run gen:recordings` in `app/`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use term::cast;
use term::replay::{self, TimedFrame};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn replayed(name: &str) -> Vec<TimedFrame> {
    let path = root().join(format!("crates/term/recordings/{name}.cast"));
    let text = std::fs::read_to_string(&path).unwrap();
    let (h, ev) = cast::parse(&text).unwrap();
    replay::replay(h, &ev, replay::CELL).unwrap()
}

/// The screen as text after each frame.
fn screens(frames: &[TimedFrame]) -> Vec<Vec<String>> {
    let mut grid: BTreeMap<u16, String> = BTreeMap::new();
    let mut out = Vec::new();
    for tf in frames {
        let f = &tf.frame;
        if f.full {
            grid.clear();
        }
        for row in &f.rows {
            let mut s = String::new();
            for run in &row.runs {
                let wide = run.wide.as_deref().unwrap_or(&[]);
                for i in 0..run.len {
                    let tail = i > 0 && wide.contains(&(i - 1));
                    match run.text.as_ref().map(|t| t[i as usize].as_str()) {
                        Some(c) if !c.is_empty() => s.push_str(c),
                        _ if tail => {}
                        _ => s.push(' '),
                    }
                }
            }
            grid.insert(row.y, s.trim_end().to_owned());
        }
        out.push(grid.values().cloned().collect());
    }
    out
}

const NAMES: [&str; 3] = ["vim", "man", "sync"];

#[test]
fn frame_files_match_recordings() {
    let update = std::env::var_os("UPDATE_RECORDINGS").is_some();
    let mut stale = Vec::new();
    for name in NAMES {
        let jsonl = replay::to_jsonl(&replayed(name));
        let path = root().join(format!("app/src/dev/recordings/{name}.jsonl"));
        if update {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, jsonl).unwrap();
        } else if std::fs::read_to_string(&path).ok().as_deref() != Some(jsonl.as_str()) {
            stale.push(name);
        }
    }
    assert!(
        stale.is_empty(),
        "stale frame files {stale:?}: run `npm run gen:recordings` in app/"
    );
}

#[test]
fn frames_are_well_formed() {
    for name in NAMES {
        let frames = replayed(name);
        assert!(frames[0].frame.full, "{name}: first frame is full");
        for (i, w) in frames.windows(2).enumerate() {
            assert_eq!(w[1].frame.seq, w[0].frame.seq + 1, "{name}: seq at {i}");
            assert!(w[1].t >= w[0].t, "{name}: time goes forward at {i}");
        }
        for tf in &frames {
            let f = &tf.frame;
            for row in &f.rows {
                let covered: u16 = row.runs.iter().map(|r| r.len).sum();
                assert_eq!(
                    covered, f.cols,
                    "{name}: seq {} row {} covers the width",
                    f.seq, row.y
                );
                for r in &row.runs {
                    assert!((r.s as usize) < f.styles.len());
                    if let Some(t) = &r.text {
                        assert_eq!(t.len(), r.len as usize);
                    }
                }
            }
        }
    }
}

#[test]
fn vim_shows_highlighted_split_with_wide_text() {
    let frames = replayed("vim");
    let screens = screens(&frames);
    let last = screens.last().unwrap();
    let all = last.join("\n");
    assert!(all.contains("fn area(self) -> f64"), "{all}");
    assert!(all.contains("-- VISUAL LINE --"), "{all}");
    // Two windows side by side: two status lines on one row.
    assert!(
        last.iter().any(|l| l.matches("sample.rs").count() == 2),
        "{all}"
    );
    let first = screens
        .iter()
        .find(|s| s.iter().any(|l| l.contains("pub fn word_counts")))
        .unwrap();
    assert!(
        first
            .iter()
            .any(|l| l.contains("日本語のコメントと絵文字 🦀")),
        "{}",
        first.join("\n")
    );
    // Truecolor from habamax: more than a handful of styles somewhere.
    assert!(frames.iter().any(|f| f.frame.styles.len() > 8));
}

#[test]
fn man_page_has_bold_and_underline() {
    let frames = replayed("man");
    let styles: Vec<_> = frames.iter().flat_map(|f| f.frame.styles.iter()).collect();
    assert!(styles.iter().any(|s| s.flags & proto::flags::BOLD != 0));
    assert!(
        styles
            .iter()
            .any(|s| s.flags & proto::flags::UNDERLINE_MASK != 0)
    );
    let last = screens(&frames).pop().unwrap().join("\n");
    assert!(last.contains("-G      Enable colorized output."), "{last}");
}

#[test]
fn synchronized_output_never_shows_a_half_drawn_screen() {
    let frames = replayed("sync");
    let screens = screens(&frames);
    let mut seen = 0;
    for (tf, s) in frames.iter().zip(&screens) {
        let top = s.iter().any(|l| l.starts_with('┌'));
        let bottom = s.iter().any(|l| l.starts_with('└'));
        assert_eq!(
            top,
            bottom,
            "seq {} shows a half-drawn screen:\n{}",
            tf.frame.seq,
            s.join("\n")
        );
        if let Some(l) = s.iter().find(|l| l.contains("frame ")) {
            let n: u32 = l.split_whitespace().nth(2).unwrap().parse().unwrap();
            assert!(n >= seen, "frames go forward");
            seen = n;
        }
    }
    assert_eq!(seen, 6, "the last frame is shown");
}
