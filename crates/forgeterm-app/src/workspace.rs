//! Tabs and split layout. Pure state (no rendering), unit-tested.

use iced::widget::pane_grid::{self, Axis, Direction, Pane};

use crate::terminal_pane::TermId;

/// One tab: a tree of split panes, each showing one terminal.
pub struct Tab {
    pub panes: pane_grid::State<TermId>,
    pub focus: Pane,
}

impl Tab {
    pub fn new(id: TermId) -> Self {
        let (panes, focus) = pane_grid::State::new(id);
        Self { panes, focus }
    }

    pub fn focused(&self) -> Option<TermId> {
        self.panes.get(self.focus).copied()
    }

    pub fn terminals(&self) -> impl Iterator<Item = TermId> + '_ {
        self.panes.iter().map(|(_, id)| *id)
    }

    pub fn find(&self, id: TermId) -> Option<Pane> {
        self.panes
            .iter()
            .find_map(|(pane, t)| (*t == id).then_some(*pane))
    }

    /// Splits the focused pane; the new terminal gets focus.
    /// `Axis::Vertical` = side by side, `Axis::Horizontal` = stacked.
    pub fn split(&mut self, axis: Axis, id: TermId) -> Option<Pane> {
        let (pane, _) = self.panes.split(axis, self.focus, id)?;
        self.focus = pane;
        Some(pane)
    }

    pub fn focus_adjacent(&mut self, direction: Direction) -> bool {
        match self.panes.adjacent(self.focus, direction) {
            Some(pane) => {
                self.focus = pane;
                true
            }
            None => false,
        }
    }
}

#[derive(Default)]
pub struct Workspace {
    pub tabs: Vec<Tab>,
    pub active: usize,
}

impl Workspace {
    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }

    pub fn focused(&self) -> Option<TermId> {
        self.active_tab().and_then(Tab::focused)
    }

    /// Adds a tab after the last one and activates it.
    pub fn push_tab(&mut self, id: TermId) {
        self.tabs.push(Tab::new(id));
        self.active = self.tabs.len() - 1;
    }

    pub fn select(&mut self, index: usize) -> bool {
        if index < self.tabs.len() {
            self.active = index;
            true
        } else {
            false
        }
    }

    /// Cycles through tabs; `forward = false` goes left. Wraps around.
    pub fn cycle(&mut self, forward: bool) {
        let n = self.tabs.len();
        if n > 0 {
            self.active = if forward {
                (self.active + 1) % n
            } else {
                (self.active + n - 1) % n
            };
        }
    }

    pub fn locate(&self, id: TermId) -> Option<(usize, Pane)> {
        self.tabs
            .iter()
            .enumerate()
            .find_map(|(i, tab)| tab.find(id).map(|pane| (i, pane)))
    }

    pub fn is_visible(&self, id: TermId) -> bool {
        self.active_tab().is_some_and(|tab| tab.find(id).is_some())
    }

    /// Removes a terminal's pane; drops its tab when it was the last pane.
    /// Returns `false` if the terminal was not found.
    pub fn remove_terminal(&mut self, id: TermId) -> bool {
        let Some((tab_index, pane)) = self.locate(id) else {
            return false;
        };
        let tab = &mut self.tabs[tab_index];
        if tab.panes.len() > 1 {
            if let Some((_, sibling)) = tab.panes.close(pane)
                && tab.focus == pane
            {
                tab.focus = sibling;
            }
        } else {
            self.remove_tab(tab_index);
        }
        true
    }

    /// Removes a whole tab and returns the terminals it contained.
    pub fn remove_tab(&mut self, index: usize) -> Vec<TermId> {
        if index >= self.tabs.len() {
            return Vec::new();
        }
        let tab = self.tabs.remove(index);
        if self.active > index || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        tab.terminals().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_push_select_and_cycle() {
        let mut ws = Workspace::default();
        ws.push_tab(1);
        ws.push_tab(2);
        ws.push_tab(3);
        assert_eq!(ws.active, 2);
        assert_eq!(ws.focused(), Some(3));
        ws.cycle(true);
        assert_eq!(ws.active, 0);
        ws.cycle(false);
        assert_eq!(ws.active, 2);
        assert!(ws.select(1));
        assert!(!ws.select(9));
        assert_eq!(ws.focused(), Some(2));
    }

    #[test]
    fn split_focuses_new_pane_and_close_returns_focus_to_sibling() {
        let mut ws = Workspace::default();
        ws.push_tab(1);
        let tab = ws.active_tab_mut().unwrap();
        tab.split(Axis::Vertical, 2).unwrap();
        assert_eq!(tab.focused(), Some(2));
        tab.split(Axis::Horizontal, 3).unwrap();
        assert_eq!(tab.panes.len(), 3);

        assert!(ws.remove_terminal(3));
        let tab = ws.active_tab().unwrap();
        assert_eq!(tab.panes.len(), 2);
        assert_eq!(tab.focused(), Some(2));
        assert!(ws.is_visible(1) && ws.is_visible(2) && !ws.is_visible(3));
    }

    #[test]
    fn focus_moves_between_adjacent_panes() {
        let mut tab = Tab::new(1);
        tab.split(Axis::Vertical, 2).unwrap();
        assert!(tab.focus_adjacent(Direction::Left));
        assert_eq!(tab.focused(), Some(1));
        assert!(!tab.focus_adjacent(Direction::Left));
        assert!(tab.focus_adjacent(Direction::Right));
        assert_eq!(tab.focused(), Some(2));
    }

    #[test]
    fn closing_last_pane_removes_tab_and_keeps_active_index_valid() {
        let mut ws = Workspace::default();
        ws.push_tab(1);
        ws.push_tab(2);
        ws.push_tab(3);
        ws.select(2);
        assert!(ws.remove_terminal(3));
        assert_eq!(ws.tabs.len(), 2);
        assert_eq!(ws.focused(), Some(2));

        ws.select(0);
        assert!(ws.remove_terminal(1));
        assert_eq!(ws.focused(), Some(2));

        assert!(ws.remove_terminal(2));
        assert!(ws.tabs.is_empty());
        assert_eq!(ws.focused(), None);
        assert!(!ws.remove_terminal(2));
    }

    #[test]
    fn removing_a_tab_left_of_active_keeps_the_same_tab_active() {
        let mut ws = Workspace::default();
        ws.push_tab(1);
        let tab = ws.active_tab_mut().unwrap();
        tab.split(Axis::Vertical, 4).unwrap();
        ws.push_tab(2);
        ws.push_tab(3);
        ws.select(2);
        // Pane iteration order is unspecified; compare as a sorted list.
        let mut removed = ws.remove_tab(0);
        removed.sort_unstable();
        assert_eq!(removed, vec![1, 4]);
        assert_eq!(ws.focused(), Some(3));
    }
}
