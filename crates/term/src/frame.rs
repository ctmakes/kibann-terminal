//! Render state to `proto::Frame`.
//!
//! Mirrors `app/src/dev/screen.ts` `buildFrame`: one style table per frame,
//! runs split on style changes, `text: None` for all-blank runs, and `wide`
//! offsets for wide heads (whose tail entry is "").

use std::collections::HashMap;

use proto::{Cursor, CursorStyle, Frame, Link, MouseTracking, Row, Run, Scrollbar, Style, flags};

use crate::vt::Terminal;
use crate::vt::color::Rgb;
use crate::vt::error::Result;
use crate::vt::mode::Mode;
use crate::vt::render::{CellIterator, Colors, CursorVisualStyle, Dirty, RenderState, RowIterator};
use crate::vt::screen::CellWide;
use crate::vt::style::{StyleColor, Underline};

fn rgb(c: Rgb) -> u32 {
    (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)
}

fn style_color(c: StyleColor, colors: &Colors) -> Option<u32> {
    match c {
        StyleColor::None => None,
        StyleColor::Palette(i) => Some(rgb(colors.palette[usize::from(i.0)])),
        StyleColor::Rgb(c) => Some(rgb(c)),
    }
}

fn underline_bits(u: Underline) -> u16 {
    let v: u16 = match u {
        Underline::Single => 1,
        Underline::Double => 2,
        Underline::Curly => 3,
        Underline::Dotted => 4,
        Underline::Dashed => 5,
        _ => 0,
    };
    v << flags::UNDERLINE_SHIFT
}

/// Everything in a frame besides rows. A change here alone still sends a frame.
#[derive(Clone, PartialEq)]
struct Meta {
    cols: u16,
    rows: u16,
    fg: u32,
    bg: u32,
    cursor: Cursor,
    mouse_tracking: MouseTracking,
    focus_reporting: bool,
    scrollbar: Scrollbar,
}

pub fn mouse_tracking(term: &Terminal<'_, '_>) -> MouseTracking {
    let on = |m| term.mode(m).unwrap_or(false);
    if on(Mode::ANY_MOUSE) {
        MouseTracking::Any
    } else if on(Mode::BUTTON_MOUSE) {
        MouseTracking::Button
    } else if on(Mode::NORMAL_MOUSE) || on(Mode::X10_MOUSE) {
        MouseTracking::Normal
    } else {
        MouseTracking::None
    }
}

#[derive(Clone, Copy)]
struct CellOut {
    style: Style,
    wide: bool,
    /// Byte range of the cell's text in the row's text buffer.
    text: (u32, u32),
}

pub struct FrameBuilder {
    state: RenderState<'static>,
    rows: RowIterator<'static>,
    cells: CellIterator<'static>,
    seq: u64,
    last: Option<Meta>,
    // Scratch buffers reused across rows.
    row_cells: Vec<CellOut>,
    row_text: String,
    grapheme: String,
    /// Selected text's colors (foreground, background); `None` inverts.
    pub selection: Option<(Option<u32>, u32)>,
}

impl FrameBuilder {
    pub fn new() -> Result<Self> {
        Ok(Self {
            state: RenderState::new()?,
            rows: RowIterator::new()?,
            cells: CellIterator::new()?,
            seq: 0,
            last: None,
            row_cells: Vec::new(),
            row_text: String::new(),
            grapheme: String::new(),
            selection: None,
        })
    }

    /// Seq of the last frame built (0 before the first).
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// Update from the terminal and build a frame of what changed, then mark
    /// the render state clean. `None` when nothing changed and there is
    /// nothing in `extra` to deliver.
    pub fn build(
        &mut self,
        term: &Terminal<'static, '_>,
        force_full: bool,
        extra: Extra,
    ) -> Result<Option<Frame>> {
        let Extra {
            selection_text,
            title,
            link,
        } = extra;
        let mouse_tracking = mouse_tracking(term);
        let sb = term.scrollbar()?;
        let clamp = |v: u64| u32::try_from(v).unwrap_or(u32::MAX);
        let scrollbar = Scrollbar {
            total: clamp(sb.total),
            offset: clamp(sb.offset),
            len: clamp(sb.len),
        };
        let focus_reporting = term.mode(Mode::FOCUS_EVENT).unwrap_or(false);

        let snap = self.state.update(term)?;
        let dirty = snap.dirty()?;
        let colors = snap.colors()?;
        let selection = self.selection;
        let cols = snap.cols()?;
        let nrows = snap.rows()?;
        let default_fg = rgb(colors.foreground);
        let default_bg = rgb(colors.background);
        let c = snap.cursor()?;
        let (cx, cy, cursor_tail) = match c.viewport {
            Some(v) if v.at_wide_tail && v.x > 0 => (v.x - 1, v.y, true),
            Some(v) => (v.x, v.y, false),
            None => (0, 0, false),
        };
        let mut cursor = Cursor {
            x: cx,
            y: cy,
            in_viewport: c.viewport.is_some(),
            visible: c.visible,
            blinking: c.blinking,
            style: match c.visual_style {
                CursorVisualStyle::Bar => CursorStyle::Bar,
                CursorVisualStyle::Underline => CursorStyle::Underline,
                CursorVisualStyle::BlockHollow => CursorStyle::BlockHollow,
                _ => CursorStyle::Block,
            },
            wide: cursor_tail,
            color: colors.cursor.map(rgb),
        };

        let full = force_full
            || dirty == Dirty::Full
            || self.last.as_ref().is_none_or(|m| {
                m.cols != cols || m.rows != nrows || m.fg != default_fg || m.bg != default_bg
            });

        let default_style = Style {
            fg: default_fg,
            bg: None,
            ul: None,
            flags: 0,
        };
        let mut styles: Vec<Style> = Vec::new();
        let mut index: HashMap<Style, u16> = HashMap::new();
        let mut out_rows: Vec<Row> = Vec::new();

        let mut it = self.rows.update(&snap)?;
        let mut y: u16 = 0;
        while let Some(row) = it.next() {
            let is_cursor_row = cursor.in_viewport && y == cy;
            let row_dirty = full || row.dirty()?;
            if row_dirty || (is_cursor_row && !cursor_tail) {
                self.row_cells.clear();
                self.row_text.clear();
                // Selected cells take the window's colors swapped, like
                // Ghostty without selection colors configured.
                let selected = row.selection()?.map(|r| r.start_x..=r.end_x);
                let mut ci = self.cells.update(row)?;
                let mut prev = default_style;
                while let Some(cell) = ci.next() {
                    let wide = cell.raw_cell()?.wide()?;
                    let start = self.row_text.len() as u32;
                    let style = if wide == CellWide::SpacerTail {
                        // Keep the tail in its head's run.
                        prev
                    } else {
                        cell.graphemes_utf8(&mut self.grapheme)?;
                        if wide != CellWide::SpacerHead && self.grapheme != " " {
                            self.row_text.push_str(&self.grapheme);
                        }
                        let fg = cell.fg_color()?.map(rgb).unwrap_or(default_fg);
                        let bg = cell.bg_color()?.map(rgb);
                        let (mut fl, mut ul, mut inverse) = (0u16, None, false);
                        if cell.has_styling()? {
                            let s = cell.style()?;
                            inverse = s.inverse;
                            ul = style_color(s.underline_color, &colors);
                            if s.bold {
                                fl |= flags::BOLD
                            }
                            if s.italic {
                                fl |= flags::ITALIC
                            }
                            if s.faint {
                                fl |= flags::FAINT
                            }
                            if s.invisible {
                                fl |= flags::INVISIBLE
                            }
                            if s.strikethrough {
                                fl |= flags::STRIKE
                            }
                            if s.overline {
                                fl |= flags::OVERLINE
                            }
                            fl |= underline_bits(s.underline);
                        }
                        if inverse {
                            Style {
                                fg: bg.unwrap_or(default_bg),
                                bg: Some(fg),
                                ul,
                                flags: fl,
                            }
                        } else {
                            Style {
                                fg,
                                bg,
                                ul,
                                flags: fl,
                            }
                        }
                    };
                    let x = self.row_cells.len() as u16;
                    let style = match &selected {
                        Some(r) if r.contains(&x) && wide != CellWide::SpacerTail => {
                            match selection {
                                Some((fg, bg)) => Style {
                                    fg: fg.unwrap_or(style.fg),
                                    bg: Some(bg),
                                    ..style
                                },
                                None => Style {
                                    fg: default_bg,
                                    bg: Some(default_fg),
                                    ..style
                                },
                            }
                        }
                        _ => style,
                    };
                    prev = style;
                    let end = self.row_text.len() as u32;
                    self.row_cells.push(CellOut {
                        style,
                        wide: wide == CellWide::Wide,
                        text: (start, end),
                    });
                }
                if is_cursor_row && !cursor_tail {
                    cursor.wide = self.row_cells.get(usize::from(cx)).is_some_and(|c| c.wide);
                }
                if row_dirty {
                    let runs = runs(&self.row_cells, &self.row_text, &mut styles, &mut index);
                    out_rows.push(Row { y, runs });
                }
            }
            y += 1;
        }

        let meta = Meta {
            cols,
            rows: nrows,
            fg: default_fg,
            bg: default_bg,
            cursor: cursor.clone(),
            mouse_tracking,
            focus_reporting,
            scrollbar,
        };
        if !full
            && out_rows.is_empty()
            && selection_text.is_none()
            && title.is_none()
            && link.is_none()
            && self.last.as_ref() == Some(&meta)
        {
            snap.clean()?;
            return Ok(None);
        }
        snap.clean()?;
        self.last = Some(meta);
        self.seq += 1;
        Ok(Some(Frame {
            seq: self.seq,
            full,
            cols,
            rows_total: nrows,
            default_fg,
            default_bg,
            styles,
            rows: out_rows,
            cursor,
            mouse_tracking,
            focus_reporting,
            scrollbar,
            selection_text,
            title,
            link,
        }))
    }
}

/// Changes that go out with the next frame even when the screen didn't
/// change. See the fields of the same names on [`Frame`].
#[derive(Debug, Default)]
pub struct Extra {
    pub selection_text: Option<String>,
    pub title: Option<String>,
    pub link: Option<Link>,
}

/// Split a row into runs of one style each.
fn runs(
    cells: &[CellOut],
    row_text: &str,
    styles: &mut Vec<Style>,
    index: &mut HashMap<Style, u16>,
) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut x = 0;
    while x < cells.len() {
        let style = cells[x].style;
        let mut end = x + 1;
        while end < cells.len() && cells[end].style == style {
            end += 1;
        }
        let s = *index.entry(style).or_insert_with(|| {
            styles.push(style);
            (styles.len() - 1) as u16
        });
        let span = &cells[x..end];
        let text = span.iter().any(|c| c.text.0 != c.text.1).then(|| {
            span.iter()
                .map(|c| row_text[c.text.0 as usize..c.text.1 as usize].to_owned())
                .collect()
        });
        let wide: Vec<u16> = span
            .iter()
            .enumerate()
            .filter(|(_, c)| c.wide)
            .map(|(i, _)| i as u16)
            .collect();
        runs.push(Run {
            x: x as u16,
            len: (end - x) as u16,
            s,
            text,
            wide: (!wide.is_empty()).then_some(wide),
        });
        x = end;
    }
    runs
}
