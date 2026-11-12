//! The terminal emulator without any I/O: VT in, frames and PTY bytes out.
//!
//! libghostty's terminal and render state can't leave the thread that made
//! them, so an `Emulator` lives on the session thread and is driven from there.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use proto::{Frame, InputEvent, KeyAction, MouseAction, Resize, ScrollBy};

use crate::frame::FrameBuilder;
use crate::keymap::{key_from_code, unshifted_codepoint};
use crate::selection::Selector;
use crate::vt::error::{Error, Result};
use crate::vt::key::{self, Mods};
use crate::vt::mode::Mode;
use crate::vt::screen::Screen;
use crate::vt::terminal::ScrollViewport;
use crate::vt::{Terminal, focus, mouse, paste};

/// How long a program may hold rendering (mode 2026) before we end the hold.
pub const HOLD_TIMEOUT: Duration = Duration::from_secs(1);

pub const TERMINFO_NAME: &str = "xterm-256color";
pub const SCROLLBACK_LINES: usize = 10_000;

/// State the terminal's callbacks write to.
struct Shared {
    builder: RefCell<FrameBuilder>,
    /// Bytes for the PTY: device responses and encoded input.
    outbox: RefCell<Vec<u8>>,
    held: Cell<bool>,
    hold_since: Cell<Option<Instant>>,
    /// The frame captured when a hold began, waiting to be sent.
    captured: RefCell<Option<Frame>>,
}

pub struct Emulator {
    term: Terminal<'static, 'static>,
    shared: Rc<Shared>,
    key_enc: key::Encoder<'static>,
    key_ev: key::Event<'static>,
    mouse_enc: mouse::Encoder<'static>,
    mouse_ev: mouse::Event<'static>,
    /// Mouse buttons held, as a bitmask of `1 << button`.
    buttons: u32,
    size: Resize,
    sel: Selector,
    /// Name of the foreground program, the title when the program sets none.
    program_name: Option<String>,
    /// The title the frontend last received.
    sent_title: String,
}

/// Keys that only change modifiers; pressing them isn't typing.
fn is_modifier(code: &str) -> bool {
    matches!(
        code,
        "ShiftLeft"
            | "ShiftRight"
            | "ControlLeft"
            | "ControlRight"
            | "AltLeft"
            | "AltRight"
            | "MetaLeft"
            | "MetaRight"
            | "OSLeft"
            | "OSRight"
            | "CapsLock"
            | "NumLock"
            | "Fn"
            | "FnLock"
    )
}

fn mods(bits: u16) -> Mods {
    Mods::from_bits_truncate(bits)
}

fn mouse_button(b: u8) -> mouse::Button {
    use mouse::Button::*;
    match b {
        1 => Left,
        2 => Right,
        3 => Middle,
        4 => Four,
        5 => Five,
        6 => Six,
        7 => Seven,
        8 => Eight,
        9 => Nine,
        10 => Ten,
        11 => Eleven,
        _ => Unknown,
    }
}

impl Emulator {
    pub fn new(size: Resize) -> Result<Self> {
        let shared = Rc::new(Shared {
            builder: RefCell::new(FrameBuilder::new()?),
            outbox: RefCell::new(Vec::new()),
            held: Cell::new(false),
            hold_since: Cell::new(None),
            captured: RefCell::new(None),
        });

        let mut term = Terminal::new(size.cols.max(1), size.rows.max(1))?;
        term.set_scrollback_max_lines(Some(SCROLLBACK_LINES))?
            .set_terminfo_name(TERMINFO_NAME)?;
        term.resize(
            size.cols.max(1),
            size.rows.max(1),
            u32::from(size.cell_w),
            u32::from(size.cell_h),
        )?;

        let s = shared.clone();
        term.on_pty_write(move |_t, data| s.outbox.borrow_mut().extend_from_slice(data))?;
        let s = shared.clone();
        term.on_render_hold(move |t, held| {
            if held {
                // Capture the frame the program wants left on screen while it
                // draws the next one. If an earlier capture is still unsent,
                // keep that one: it is just as consistent.
                if s.captured.borrow().is_none()
                    && let Ok(mut b) = s.builder.try_borrow_mut()
                    && let Ok(Some(f)) = b.build(t, false, None, None)
                {
                    *s.captured.borrow_mut() = Some(f);
                }
                s.hold_since.set(Some(Instant::now()));
            } else {
                s.hold_since.set(None);
            }
            s.held.set(held);
        })?;

        Ok(Self {
            term,
            shared,
            key_enc: key::Encoder::new()?,
            key_ev: key::Event::new()?,
            mouse_enc: mouse::Encoder::new()?,
            mouse_ev: mouse::Event::new()?,
            buttons: 0,
            size,
            sel: Selector::new()?,
            program_name: None,
            sent_title: String::new(),
        })
    }

    pub fn terminal(&self) -> &Terminal<'static, 'static> {
        &self.term
    }

    pub fn size(&self) -> Resize {
        self.size
    }

    /// Feed program output.
    pub fn write(&mut self, data: &[u8]) {
        self.term.vt_write(data);
    }

    /// Take the bytes waiting to go to the PTY.
    pub fn take_outbox(&self) -> Vec<u8> {
        std::mem::take(&mut *self.shared.outbox.borrow_mut())
    }

    pub fn held(&self) -> bool {
        self.shared.held.get()
    }

    /// When the current hold times out, if there is one.
    /// The directory the shell last reported (OSC 7), if any.
    pub fn reported_dir(&self) -> Option<std::path::PathBuf> {
        crate::process::parse_reported_dir(self.term.pwd().ok()?)
    }

    pub fn hold_deadline(&self) -> Option<Instant> {
        self.shared
            .hold_since
            .get()
            .filter(|_| self.held())
            .map(|t| t + HOLD_TIMEOUT)
    }

    /// End a hold that has gone on too long. Returns true if one was ended.
    pub fn expire_hold(&mut self, now: Instant) -> bool {
        match self.hold_deadline() {
            Some(d) if now >= d => {
                self.end_hold();
                true
            }
            _ => false,
        }
    }

    /// End the current hold now (turn mode 2026 off).
    pub fn end_hold(&mut self) {
        let _ = self.term.set_mode(Mode::SYNC_OUTPUT, false);
        self.shared.held.set(false);
        self.shared.hold_since.set(None);
    }

    /// The frame captured at the start of a hold, if one is waiting.
    pub fn take_captured(&self) -> Option<Frame> {
        self.shared.captured.borrow_mut().take()
    }

    pub fn has_captured(&self) -> bool {
        self.shared.captured.borrow().is_some()
    }

    /// Build a frame of everything that changed since the last one.
    pub fn build_frame(&mut self, force_full: bool) -> Result<Option<Frame>> {
        let mut text = self.sel.take_pending(&self.term);
        // A requested full frame replaces frames the frontend dropped, which
        // may have carried the text.
        if force_full && text.is_none() {
            text = Some(self.sel.sent_text().to_owned());
        }
        let title = self.title();
        let title = (force_full || title != self.sent_title).then(|| {
            self.sent_title.clone_from(&title);
            title
        });
        self.shared
            .builder
            .borrow_mut()
            .build(&self.term, force_full, text, title)
    }

    /// The program's title (OSC 0/2) if it set one, else the foreground
    /// program's name from [`Self::set_program_name`].
    pub fn title(&self) -> String {
        match self.term.title() {
            Ok(t) if !t.is_empty() => String::from_utf8_lossy(t).into_owned(),
            _ => self.program_name.clone().unwrap_or_default(),
        }
    }

    /// The foreground program's name, the title when the program sets none.
    pub fn set_program_name(&mut self, name: Option<String>) {
        self.program_name = name;
    }

    /// A selection drag is held past an edge; call [`Self::autoscroll_tick`]
    /// every [`crate::selection::AUTOSCROLL_INTERVAL`] until this turns false.
    pub fn autoscrolling(&self) -> bool {
        self.sel.autoscrolling(&self.term)
    }

    pub fn autoscroll_tick(&mut self) -> Result<()> {
        self.sel.autoscroll_tick(&self.term, self.size)
    }

    /// The user typed: drop the selection and jump back to the bottom.
    fn typed(&mut self) -> Result<()> {
        self.sel.clear(&self.term)?;
        self.term.scroll_viewport(ScrollViewport::Bottom);
        Ok(())
    }

    /// Position clamped to the grid, for the mouse encoder.
    fn clamp_to_grid(&self, x: f32, y: f32) -> (f32, f32) {
        let s = self.size;
        let w = f32::from(s.cols) * f32::from(s.cell_w.max(1)) - 1.0;
        let h = f32::from(s.rows) * f32::from(s.cell_h.max(1)) - 1.0;
        (x.clamp(0.0, w.max(0.0)), y.clamp(0.0, h.max(0.0)))
    }

    /// Resize the terminal. Returns false if the size didn't change.
    pub fn resize(&mut self, size: Resize) -> Result<bool> {
        if size == self.size || size.cols == 0 || size.rows == 0 {
            return Ok(false);
        }
        self.term.resize(
            size.cols,
            size.rows,
            u32::from(size.cell_w),
            u32::from(size.cell_h),
        )?;
        self.size = size;
        Ok(true)
    }

    /// Encode an input event into the outbox.
    pub fn input(&mut self, e: &InputEvent) -> Result<()> {
        let mut out = std::mem::take(&mut *self.shared.outbox.borrow_mut());
        let r = self.encode(e, &mut out);
        // Device responses may have queued meanwhile; keep them first.
        let mut ob = self.shared.outbox.borrow_mut();
        let late = std::mem::replace(&mut *ob, out);
        ob.extend_from_slice(&late);
        r
    }

    fn encode(&mut self, e: &InputEvent, out: &mut Vec<u8>) -> Result<()> {
        match e {
            InputEvent::Key {
                action,
                code,
                key: _,
                text,
                mods: m,
                composing,
            } => {
                if *action != KeyAction::Release && !composing && !is_modifier(code) {
                    self.typed()?;
                }
                let action = match action {
                    KeyAction::Press => key::Action::Press,
                    KeyAction::Repeat => key::Action::Repeat,
                    KeyAction::Release => key::Action::Release,
                };
                let ucp = unshifted_codepoint(code);
                let m = mods(*m);
                let text = text
                    .as_deref()
                    .filter(|t| !t.is_empty() && action != key::Action::Release);
                // Shift is consumed when it produced the text ('a' -> 'A').
                let consumed = if text.is_some() && ucp != '\0' {
                    m & Mods::SHIFT
                } else {
                    Mods::empty()
                };
                self.key_ev
                    .set_action(action)
                    .set_key(key_from_code(code))
                    .set_mods(m)
                    .set_consumed_mods(consumed)
                    .set_composing(*composing)
                    .set_unshifted_codepoint(ucp)
                    .set_utf8(text);
                self.key_enc.set_options_from_terminal(&self.term);
                #[cfg(target_os = "macos")]
                self.key_enc
                    .set_macos_option_as_alt(key::OptionAsAlt::False);
                self.key_enc.encode_to_vec(&self.key_ev, out)
            }
            InputEvent::Text { data } => {
                self.typed()?;
                out.extend_from_slice(data.as_bytes());
                Ok(())
            }
            InputEvent::Paste { data } => {
                self.typed()?;
                let bracketed = self.term.mode(Mode::BRACKETED_PASTE)?;
                let mut buf = vec![0u8; data.len() * 2 + 16];
                loop {
                    // The encoder edits its input in place, so each try gets a fresh copy.
                    let mut src = data.as_bytes().to_vec();
                    match paste::encode(&mut src, bracketed, &mut buf) {
                        Ok(n) => {
                            out.extend_from_slice(&buf[..n]);
                            return Ok(());
                        }
                        Err(Error::OutOfSpace { required }) => {
                            let want = required.max(buf.len() * 2);
                            buf.resize(want, 0);
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            InputEvent::Mouse {
                action,
                button,
                x,
                y,
                mods: m,
            } => {
                // Left-button gestures select text unless a program tracks
                // the mouse; Shift takes the mouse back from the program.
                let m = mods(*m);
                let tracking = self.term.is_mouse_tracking()?;
                match action {
                    MouseAction::Press
                        if *button == Some(1) && (!tracking || m.contains(Mods::SHIFT)) =>
                    {
                        return self.sel.press(&self.term, self.size, *x, *y);
                    }
                    MouseAction::Motion if self.sel.active() => {
                        return self
                            .sel
                            .drag(&self.term, self.size, *x, *y, m.contains(Mods::ALT));
                    }
                    MouseAction::Release if *button == Some(1) && self.sel.active() => {
                        return self.sel.release(&self.term, self.size, *x, *y);
                    }
                    _ if !tracking => return Ok(()),
                    MouseAction::Press => self.sel.cancel(),
                    _ => {}
                }
                let action = match action {
                    MouseAction::Press => mouse::Action::Press,
                    MouseAction::Release => mouse::Action::Release,
                    MouseAction::Motion => mouse::Action::Motion,
                };
                let (x, y) = self.clamp_to_grid(*x, *y);
                if let Some(b) = button {
                    let bit = 1u32 << (*b).min(31);
                    match action {
                        mouse::Action::Press => self.buttons |= bit,
                        mouse::Action::Release => self.buttons &= !bit,
                        _ => {}
                    }
                }
                self.prepare_mouse()?;
                self.mouse_ev
                    .set_action(action)
                    .set_button(button.map(mouse_button))
                    .set_mods(m)
                    .set_position(mouse::Position { x, y });
                self.mouse_enc.encode_to_vec(&self.mouse_ev, out)
            }
            InputEvent::Wheel {
                dx_rows,
                dy_rows,
                x,
                y,
                mods: m,
            } => self.wheel(*dx_rows, *dy_rows, *x, *y, mods(*m), out),
            InputEvent::Scroll { by, n } => {
                let rows = isize::try_from(*n).unwrap_or(0);
                self.term.scroll_viewport(match by {
                    ScrollBy::Rows => ScrollViewport::Delta(rows),
                    ScrollBy::Pages => ScrollViewport::Delta(rows * self.size.rows as isize),
                    ScrollBy::Top => ScrollViewport::Top,
                    ScrollBy::Bottom => ScrollViewport::Bottom,
                });
                Ok(())
            }
            InputEvent::Focus { focused } => {
                if !self.term.mode(Mode::FOCUS_EVENT)? {
                    return Ok(());
                }
                if !focused {
                    // Buttons released while unfocused never reach us.
                    self.buttons = 0;
                }
                let ev = if *focused {
                    focus::Event::Gained
                } else {
                    focus::Event::Lost
                };
                let mut buf = [0u8; 8];
                let n = ev.encode(&mut buf)?;
                out.extend_from_slice(&buf[..n]);
                Ok(())
            }
        }
    }

    fn prepare_mouse(&mut self) -> Result<()> {
        let s = self.size;
        let (cw, ch) = (u32::from(s.cell_w).max(1), u32::from(s.cell_h).max(1));
        self.mouse_enc
            .set_options_from_terminal(&self.term)
            .set_size(mouse::EncoderSize {
                screen_width: u32::from(s.cols) * cw,
                screen_height: u32::from(s.rows) * ch,
                cell_width: cw,
                cell_height: ch,
                padding_top: 0,
                padding_bottom: 0,
                padding_left: 0,
                padding_right: 0,
            })?
            .set_any_button_pressed(self.buttons != 0)
            .set_track_last_cell(true);
        Ok(())
    }

    fn wheel(
        &mut self,
        dx: i16,
        dy: i16,
        x: f32,
        y: f32,
        m: Mods,
        out: &mut Vec<u8>,
    ) -> Result<()> {
        if self.term.is_mouse_tracking()? {
            let (x, y) = self.clamp_to_grid(x, y);
            self.prepare_mouse()?;
            let steps = [
                (dy, mouse::Button::Four, mouse::Button::Five),
                (dx, mouse::Button::Six, mouse::Button::Seven),
            ];
            for (n, neg, pos) in steps {
                let b = if n < 0 { neg } else { pos };
                // Wheel buttons report a press only, like xterm and Ghostty.
                for _ in 0..n.unsigned_abs() {
                    self.mouse_ev
                        .set_action(mouse::Action::Press)
                        .set_button(Some(b))
                        .set_mods(m)
                        .set_position(mouse::Position { x, y });
                    self.mouse_enc.encode_to_vec(&self.mouse_ev, out)?;
                }
            }
            return Ok(());
        }
        if dy == 0 {
            return Ok(());
        }
        // Primary screen: scroll through the scrollback.
        if self.term.active_screen()? != Screen::Alternate {
            self.term
                .scroll_viewport(ScrollViewport::Delta(isize::from(dy)));
            return Ok(());
        }
        // Alternate screen (no scrollback): arrows, if the program allows it.
        if !self.term.mode(Mode::ALT_SCROLL)? {
            return Ok(());
        }
        let k = if dy < 0 {
            key::Key::ArrowUp
        } else {
            key::Key::ArrowDown
        };
        self.key_enc.set_options_from_terminal(&self.term);
        for _ in 0..dy.unsigned_abs() {
            self.key_ev
                .set_action(key::Action::Press)
                .set_key(k)
                .set_mods(Mods::empty())
                .set_consumed_mods(Mods::empty())
                .set_composing(false)
                .set_unshifted_codepoint('\0')
                .set_utf8(None::<String>);
            self.key_enc.encode_to_vec(&self.key_ev, out)?;
        }
        Ok(())
    }
}
