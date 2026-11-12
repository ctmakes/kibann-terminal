//! The workspace layout the daemon owns: tabs of split trees, which pane has
//! focus in each, and which tab is shown. Pure data; spawning and closing
//! shells is the server's job. Mirrors `app/src/workspace/model.ts`, which
//! the browser harness uses in place of the daemon.
//!
//! Geometry (pixels, dividers, which pane is to the left) stays in the app:
//! the daemon never knows the window's size.

use proto::layout::{Dir, Layout, PaneId, Step, Tab, TabId, Tree};

/// Smallest share of a split either side may shrink to.
const MIN_RATIO: f64 = 0.05;

#[derive(Debug, Default)]
pub struct Model {
    layout: Layout,
    next_tab: TabId,
}

/// Replace the leaf for `target` with what `f` makes of it. True if found.
fn replace_leaf(t: &mut Tree, target: PaneId, f: &mut dyn FnMut(Tree) -> Tree) -> bool {
    match t {
        Tree::Leaf { pane } if *pane == target => {
            let old = std::mem::replace(t, Tree::leaf(0));
            *t = f(old);
            true
        }
        Tree::Leaf { .. } => false,
        Tree::Split { a, b, .. } => replace_leaf(a, target, f) || replace_leaf(b, target, f),
    }
}

/// Remove `pane`; its sibling takes the space. `None` when it was the last.
fn remove(t: Tree, pane: PaneId) -> Option<Tree> {
    match t {
        Tree::Leaf { pane: p } if p == pane => None,
        Tree::Leaf { .. } => Some(t),
        Tree::Split { dir, ratio, a, b } => match (remove(*a, pane), remove(*b, pane)) {
            (Some(a), Some(b)) => Some(Tree::Split {
                dir,
                ratio,
                a: Box::new(a),
                b: Box::new(b),
            }),
            (a, b) => a.or(b),
        },
    }
}

fn node_at<'t>(t: &'t mut Tree, path: &[Step]) -> Option<&'t mut Tree> {
    let Some((step, rest)) = path.split_first() else {
        return Some(t);
    };
    match t {
        Tree::Split { a, b, .. } => node_at(if *step == Step::A { a } else { b }, rest),
        Tree::Leaf { .. } => None,
    }
}

fn equalize(t: &mut Tree) {
    fn weight(t: &Tree, d: Dir) -> u32 {
        match t {
            Tree::Split { dir, a, b, .. } if *dir == d => weight(a, d) + weight(b, d),
            _ => 1,
        }
    }
    if let Tree::Split { dir, ratio, a, b } = t {
        let (wa, wb) = (weight(a, *dir), weight(b, *dir));
        *ratio = f64::from(wa) / f64::from(wa + wb);
        equalize(a);
        equalize(b);
    }
}

impl Model {
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Start from a saved layout (tests; and the app's old saved layouts are
    /// not carried over).
    pub fn from_layout(layout: Layout) -> Self {
        let next_tab = layout.tabs.iter().map(|t| t.id).max().unwrap_or(0);
        Self { layout, next_tab }
    }

    fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.layout.tabs.iter_mut().find(|t| t.id == id)
    }

    fn tab_of_mut(&mut self, pane: PaneId) -> Option<&mut Tab> {
        self.layout.tabs.iter_mut().find(|t| t.tree.contains(pane))
    }

    /// A new tab holding `pane`, after the others. The first tab is always
    /// shown; later ones only with `focus`.
    pub fn new_tab(&mut self, pane: PaneId, focus: bool) -> TabId {
        self.next_tab += 1;
        let id = self.next_tab;
        self.layout.tabs.push(Tab {
            id,
            tree: Tree::leaf(pane),
            focused: pane,
            zoomed: None,
        });
        if focus || self.layout.active.is_none() {
            self.layout.active = Some(id);
        }
        id
    }

    /// Put `pane` right of `target` (`Row`) or below it (`Column`). Returns
    /// the tab, or `None` when `target` isn't there.
    pub fn split(&mut self, target: PaneId, dir: Dir, pane: PaneId, focus: bool) -> Option<TabId> {
        let tab = self.tab_of_mut(target)?;
        replace_leaf(&mut tab.tree, target, &mut |old| Tree::Split {
            dir,
            ratio: 0.5,
            a: Box::new(old),
            b: Box::new(Tree::leaf(pane)),
        });
        tab.zoomed = None;
        let id = tab.id;
        if focus {
            tab.focused = pane;
            self.layout.active = Some(id);
        }
        Some(id)
    }

    /// Take `pane` out. Focus moves to the pane before it in reading order;
    /// an emptied tab goes, and the tab after it (else before) is shown.
    /// False when it wasn't there.
    pub fn remove(&mut self, pane: PaneId) -> bool {
        let Some(i) = self.layout.tabs.iter().position(|t| t.tree.contains(pane)) else {
            return false;
        };
        let tab = &mut self.layout.tabs[i];
        let order = tab.tree.panes();
        let at = order.iter().position(|&p| p == pane).unwrap_or(0);
        let tree = std::mem::replace(&mut tab.tree, Tree::leaf(0));
        match remove(tree, pane) {
            Some(tree) => {
                let left = tree.panes();
                tab.tree = tree;
                if tab.zoomed == Some(pane) || left.len() < 2 {
                    tab.zoomed = None;
                }
                if tab.focused == pane {
                    tab.focused = left[at.saturating_sub(1).min(left.len() - 1)];
                }
            }
            None => {
                let id = tab.id;
                self.layout.tabs.remove(i);
                if self.layout.active == Some(id) {
                    let next = self
                        .layout
                        .tabs
                        .get(i.min(self.layout.tabs.len().saturating_sub(1)));
                    self.layout.active = next.map(|t| t.id);
                }
            }
        }
        true
    }

    /// Focus `pane` and show its tab. A zoomed tab shows every pane again
    /// unless `pane` is the zoomed one.
    pub fn focus(&mut self, pane: PaneId) -> bool {
        let Some(tab) = self.tab_of_mut(pane) else {
            return false;
        };
        if tab.zoomed.is_some_and(|z| z != pane) {
            tab.zoomed = None;
        }
        tab.focused = pane;
        self.layout.active = Some(tab.id);
        true
    }

    pub fn activate(&mut self, tab: TabId) -> bool {
        if self.layout.tab(tab).is_none() {
            return false;
        }
        self.layout.active = Some(tab);
        true
    }

    pub fn set_ratio(&mut self, tab: TabId, path: &[Step], ratio: f64) -> bool {
        let Some(t) = self.tab_mut(tab) else {
            return false;
        };
        if !ratio.is_finite() {
            return false;
        }
        match node_at(&mut t.tree, path) {
            Some(Tree::Split { ratio: r, .. }) => {
                *r = ratio.clamp(MIN_RATIO, 1.0 - MIN_RATIO);
                t.zoomed = None;
                true
            }
            _ => false,
        }
    }

    pub fn equalize(&mut self, tab: TabId) -> bool {
        let Some(t) = self.tab_mut(tab) else {
            return false;
        };
        equalize(&mut t.tree);
        true
    }

    /// Show `pane` alone (and focus it), or every pane with `None`. A tab
    /// with one pane is never zoomed.
    pub fn zoom(&mut self, tab: TabId, pane: Option<PaneId>) -> bool {
        let Some(t) = self.tab_mut(tab) else {
            return false;
        };
        match pane {
            Some(p) if t.tree.contains(p) => {
                if t.tree.panes().len() > 1 {
                    t.zoomed = Some(p);
                }
                t.focused = p;
                true
            }
            Some(_) => false,
            None => {
                t.zoomed = None;
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1 | (2 over 3), in tab 1.
    fn three() -> Model {
        let mut m = Model::default();
        m.new_tab(1, true);
        m.split(1, Dir::Row, 2, true);
        m.split(2, Dir::Column, 3, true);
        m
    }

    fn tab(m: &Model, i: usize) -> &Tab {
        &m.layout().tabs[i]
    }

    #[test]
    fn splits_put_the_new_pane_right_or_below() {
        let m = three();
        let t = tab(&m, 0);
        assert_eq!(t.tree.panes(), vec![1, 2, 3]);
        assert_eq!(t.focused, 3);
        let Tree::Split { dir, b, .. } = &t.tree else {
            panic!()
        };
        assert_eq!(*dir, Dir::Row);
        assert!(matches!(
            **b,
            Tree::Split {
                dir: Dir::Column,
                ..
            }
        ));
    }

    #[test]
    fn splitting_without_focus_keeps_focus() {
        let mut m = three();
        m.focus(1);
        assert_eq!(m.split(1, Dir::Column, 4, false), Some(1));
        assert_eq!(tab(&m, 0).focused, 1);
        assert_eq!(m.split(9, Dir::Row, 5, false), None);
    }

    #[test]
    fn removing_gives_space_to_the_sibling_and_moves_focus_back() {
        let mut m = three();
        assert!(m.remove(3));
        assert_eq!(tab(&m, 0).tree.panes(), vec![1, 2]);
        assert_eq!(tab(&m, 0).focused, 2);
        assert!(m.remove(1));
        assert_eq!(tab(&m, 0).tree, Tree::leaf(2));
        assert!(!m.remove(1));
    }

    #[test]
    fn an_emptied_tab_goes_and_the_next_is_shown() {
        let mut m = three();
        let t2 = m.new_tab(4, true);
        let t3 = m.new_tab(5, false);
        assert_eq!(m.layout().active, Some(t2));
        m.remove(4);
        assert_eq!(m.layout().active, Some(t3));
        m.remove(5);
        assert_eq!(m.layout().active, Some(1));
        for p in [1, 2, 3] {
            m.remove(p);
        }
        assert_eq!(m.layout(), &Layout::default());
        // Ids are never reused.
        assert_eq!(m.new_tab(6, false), 4);
    }

    #[test]
    fn focus_shows_the_tab_and_ends_zoom_on_another_pane() {
        let mut m = three();
        let t2 = m.new_tab(4, true);
        m.zoom(1, Some(2));
        assert_eq!(tab(&m, 0).zoomed, Some(2));
        assert_eq!(tab(&m, 0).focused, 2);
        m.focus(2);
        assert_eq!(tab(&m, 0).zoomed, Some(2));
        assert_eq!(m.layout().active, Some(1));
        m.focus(3);
        assert_eq!(tab(&m, 0).zoomed, None);
        assert!(m.activate(t2));
        assert!(!m.activate(99));
    }

    #[test]
    fn ratios_are_clamped_and_equalize_shares_runs() {
        let mut m = three();
        assert!(m.set_ratio(1, &[], 2.0));
        assert!(m.set_ratio(1, &[Step::B], 0.2));
        assert!(!m.set_ratio(1, &[Step::A], 0.2)); // a leaf
        let Tree::Split { ratio, .. } = &tab(&m, 0).tree else {
            panic!()
        };
        assert!((ratio - 0.95).abs() < 1e-9);

        // 1 | 2 | 3 in a row: each a third.
        let mut m = Model::default();
        m.new_tab(1, true);
        m.split(1, Dir::Row, 2, true);
        m.split(2, Dir::Row, 3, true);
        m.equalize(1);
        let Tree::Split { ratio, b, .. } = &tab(&m, 0).tree else {
            panic!()
        };
        assert!((ratio - 1.0 / 3.0).abs() < 1e-9);
        let Tree::Split { ratio, .. } = &**b else {
            panic!()
        };
        assert!((ratio - 0.5).abs() < 1e-9);
    }

    #[test]
    fn zoom_needs_two_panes() {
        let mut m = Model::default();
        m.new_tab(1, true);
        assert!(m.zoom(1, Some(1)));
        assert_eq!(tab(&m, 0).zoomed, None);
        m.split(1, Dir::Row, 2, false);
        m.zoom(1, Some(2));
        assert_eq!(tab(&m, 0).zoomed, Some(2));
        m.remove(1);
        assert_eq!(tab(&m, 0).zoomed, None);
    }
}
