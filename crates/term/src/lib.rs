//! Terminal session: PTY + libghostty-vt emulator + frame builder.
//!
//! Must stay free of Tauri so it can move into the daemon unchanged.

pub use libghostty_vt as vt;

pub mod emulator;
pub mod frame;
pub mod keymap;
pub mod session;

pub use emulator::Emulator;
pub use session::{Event, Session, Spawn};

/// Version string of the linked libghostty-vt.
pub fn ghostty_version() -> String {
    vt::build_info::version_string()
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vt::render::{CellIterator, RowIterator};
    use vt::screen::CellWide;
    use vt::{RenderState, Terminal};

    type Cells = Vec<(String, CellWide, Option<(u8, u8, u8)>)>;

    /// One row read back from the render state: (text, wide, fg) per cell.
    fn read_row(terminal: &Terminal<'_, '_>, y: usize) -> Cells {
        let mut state = RenderState::new().unwrap();
        let mut rows = RowIterator::new().unwrap();
        let mut cells = CellIterator::new().unwrap();
        let snapshot = state.update(terminal).unwrap();
        let mut row_iter = rows.update(&snapshot).unwrap();
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(row) = row_iter.next() {
            if i == y {
                let mut cell_iter = cells.update(row).unwrap();
                let mut text = String::new();
                while let Some(cell) = cell_iter.next() {
                    cell.graphemes_utf8(&mut text).unwrap();
                    let wide = cell.raw_cell().unwrap().wide().unwrap();
                    let fg = cell.fg_color().unwrap().map(|c| (c.r, c.g, c.b));
                    out.push((text.clone(), wide, fg));
                }
                break;
            }
            i += 1;
        }
        out
    }

    #[test]
    fn links_libghostty_1_0() {
        let v = ghostty_version();
        assert!(v.starts_with("1.0."), "linked libghostty-vt {v}");
    }

    #[test]
    fn round_trips_styled_and_wide_text() {
        let mut t = Terminal::new(20, 3).unwrap();
        t.vt_write("\x1b[31mred\x1b[0m 日x".as_bytes());
        let row = read_row(&t, 0);

        assert_eq!(row[0].0, "r");
        assert!(row[0].2.is_some(), "red text should have its own fg color");
        // A written space keeps its text; only never-written cells are empty.
        assert_eq!(row[3].0, " ");
        assert_eq!(row[3].2, None, "reset text uses the default fg");
        assert_eq!(row[7].0, "");

        assert_eq!(row[4].0, "日");
        assert_eq!(row[4].1, CellWide::Wide);
        assert_eq!(row[5].1, CellWide::SpacerTail);
        assert_eq!(row[6].0, "x");
    }

    #[test]
    fn reports_partial_dirty_rows() {
        let mut t = Terminal::new(10, 4).unwrap();
        let mut state = RenderState::new().unwrap();
        let mut rows = RowIterator::new().unwrap();
        {
            let snap = state.update(&t).unwrap();
            snap.clean().unwrap();
        }
        // Move to row 3 and write. Row 3 changes, and row 1 is marked too
        // because the cursor left it (so a renderer can erase the old cursor).
        t.vt_write(b"\x1b[3;1Hhi");
        let snap = state.update(&t).unwrap();
        let mut it = rows.update(&snap).unwrap();
        let mut dirty = Vec::new();
        while let Some((y, _)) = it.next_dirty() {
            dirty.push(y);
        }
        assert_eq!(dirty, vec![0, 2]);
    }
}
