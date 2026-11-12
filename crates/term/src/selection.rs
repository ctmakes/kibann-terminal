//! Mouse selection with libghostty's gesture state machine: click to clear,
//! drag to select, double-click for words, triple-click for lines, Alt for a
//! rectangle, and autoscroll while dragging past the top or bottom edge.
//!
//! Positions are device pixels from the grid's top-left corner and may lie
//! outside the grid while dragging.

use std::time::{Duration, Instant};

use proto::Resize;

use crate::vt::Terminal;
use crate::vt::error::Result;
use crate::vt::fmt::{Format, Formatter};
use crate::vt::point::{Coordinate, Point};
use crate::vt::selection::gesture::{
    Autoscroll, AutoscrollTickEvent, DragEvent, Geometry, Gesture, PressEvent, ReleaseEvent,
};

/// Longest gap between clicks of a double or triple click.
const REPEAT_INTERVAL: Duration = Duration::from_millis(500);
/// How often to scroll one row while a drag is held past an edge.
pub const AUTOSCROLL_INTERVAL: Duration = Duration::from_millis(50);

pub struct Selector {
    gesture: Gesture<'static>,
    press: PressEvent<'static>,
    drag: DragEvent<'static>,
    release: ReleaseEvent<'static>,
    tick: AutoscrollTickEvent<'static>,
    epoch: Instant,
    /// A left-button gesture is in progress.
    active: bool,
    pointer: (f32, f32),
    rectangle: bool,
    /// The selected text the frontend last received.
    sent_text: String,
    /// Selected text that changed and still has to go out in a frame.
    pending: Option<String>,
}

/// The cell under a position, clamped to the grid.
fn cell(size: Resize, x: f32, y: f32) -> (u16, u16) {
    let clamp = |v: f32, cell: u16, n: u16| {
        let i = (v / f32::from(cell.max(1))).floor();
        i.clamp(0.0, f32::from(n.saturating_sub(1))) as u16
    };
    (
        clamp(x, size.cell_w, size.cols),
        clamp(y, size.cell_h, size.rows),
    )
}

fn geometry(size: Resize) -> Geometry {
    Geometry {
        columns: u32::from(size.cols.max(1)),
        cell_width: u32::from(size.cell_w.max(1)),
        padding_left: 0,
        screen_height: u32::from(size.rows.max(1)) * u32::from(size.cell_h.max(1)),
    }
}

fn viewport(x: u16, y: u16) -> Point {
    Point::Viewport(Coordinate { x, y: u32::from(y) })
}

impl Selector {
    pub fn new() -> Result<Self> {
        let mut press = PressEvent::new()?;
        press.set_repeat_interval(REPEAT_INTERVAL)?;
        Ok(Self {
            gesture: Gesture::new()?,
            press,
            drag: DragEvent::new()?,
            release: ReleaseEvent::new()?,
            tick: AutoscrollTickEvent::new()?,
            epoch: Instant::now(),
            active: false,
            pointer: (0.0, 0.0),
            rectangle: false,
            sent_text: String::new(),
            pending: None,
        })
    }

    /// A left-button gesture is in progress.
    pub fn active(&self) -> bool {
        self.active
    }

    /// Forget an unfinished gesture whose release went elsewhere.
    pub fn cancel(&mut self) {
        self.active = false;
    }

    pub fn press(&mut self, term: &Terminal<'_, '_>, size: Resize, x: f32, y: f32) -> Result<()> {
        let (cx, cy) = cell(size, x, y);
        let at = term.grid_ref(viewport(cx, cy))?;
        self.press
            .set_position(f64::from(x), f64::from(y))?
            .set_repeat_distance(f64::from(size.cell_w.max(4)))?
            .set_time(self.epoch.elapsed())?;
        // A single click returns no selection, which clears the old one.
        let sel = self.press.apply(&mut self.gesture, term, at)?;
        term.set_selection(sel.as_ref())?;
        self.active = true;
        self.pointer = (x, y);
        self.rectangle = false;
        Ok(())
    }

    pub fn drag(
        &mut self,
        term: &Terminal<'_, '_>,
        size: Resize,
        x: f32,
        y: f32,
        rectangle: bool,
    ) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        self.pointer = (x, y);
        self.rectangle = rectangle;
        let (cx, cy) = cell(size, x, y);
        let at = term.grid_ref(viewport(cx, cy))?;
        self.drag
            .set_position(f64::from(x), f64::from(y))?
            .set_rectangle(rectangle)?;
        if let Some(sel) = self
            .drag
            .apply(&mut self.gesture, term, at, geometry(size))?
        {
            term.set_selection(Some(&sel))?;
        }
        Ok(())
    }

    pub fn release(&mut self, term: &Terminal<'_, '_>, size: Resize, x: f32, y: f32) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        self.active = false;
        let (cx, cy) = cell(size, x, y);
        let at = term.grid_ref(viewport(cx, cy))?;
        self.release.apply(&mut self.gesture, term, Some(at))?;
        self.refresh_text(term);
        Ok(())
    }

    /// The drag is held past an edge and the viewport should keep scrolling.
    pub fn autoscrolling(&self, term: &Terminal<'_, '_>) -> bool {
        self.active && !matches!(self.gesture.autoscroll(term), Ok(Autoscroll::None) | Err(_))
    }

    /// Scroll one row toward the held edge and extend the selection.
    pub fn autoscroll_tick(&mut self, term: &Terminal<'_, '_>, size: Resize) -> Result<()> {
        if !self.autoscrolling(term) {
            return Ok(());
        }
        let (x, y) = self.pointer;
        let (cx, cy) = cell(size, x, y);
        self.tick
            .set_position(f64::from(x), f64::from(y))?
            .set_rectangle(self.rectangle)?;
        let at = Coordinate {
            x: cx,
            y: u32::from(cy),
        };
        match self
            .tick
            .apply(&mut self.gesture, term, at, geometry(size))?
        {
            Some(sel) => {
                term.set_selection(Some(&sel))?;
            }
            // The press's cell scrolled out of the scrollback: the gesture reset.
            None if self.gesture.click_count(term)? == 0 => self.active = false,
            None => {}
        }
        Ok(())
    }

    /// Clear the selection (typing does this, as in Ghostty).
    pub fn clear(&mut self, term: &Terminal<'_, '_>) -> Result<()> {
        if term.selection()?.is_some() {
            term.set_selection(None)?;
        }
        self.gesture.reset(term);
        self.active = false;
        self.refresh_text(term);
        Ok(())
    }

    /// Queue the selected text for the next frame if it changed.
    pub fn refresh_text(&mut self, term: &Terminal<'_, '_>) {
        let text = selected_text(term).unwrap_or_default();
        if text != self.sent_text {
            self.sent_text.clone_from(&text);
            self.pending = Some(text);
        }
    }

    /// The selected text as the frontend last received it.
    pub fn sent_text(&self) -> &str {
        &self.sent_text
    }

    /// Text for the next frame, if it changed. Also notices a selection the
    /// terminal dropped on its own (a cleared screen, say).
    pub fn take_pending(&mut self, term: &Terminal<'_, '_>) -> Option<String> {
        if self.pending.is_none()
            && !self.active
            && !self.sent_text.is_empty()
            && matches!(term.selection(), Ok(None))
        {
            self.sent_text.clear();
            self.pending = Some(String::new());
        }
        self.pending.take()
    }
}

/// The selection as plain text, wrapped lines joined and trailing spaces trimmed.
pub fn selected_text(term: &Terminal<'_, '_>) -> Result<String> {
    let Some(mut f) = Formatter::new_selection(term)? else {
        return Ok(String::new());
    };
    f.set_format(Format::Plain)?
        .set_unwrap(true)?
        .set_trim(true)?;
    let bytes = f.format_alloc(None)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
