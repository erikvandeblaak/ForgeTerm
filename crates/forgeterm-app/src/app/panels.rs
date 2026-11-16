//! The side panels (explorer, AI, Git, monitor): one at a time.

use super::*;

impl App {
    /// Moves the explorer to the focused shell's directory when that directory (or
    /// the focused shell) changed since the last sync.
    pub(super) fn sync_explorer(&mut self) -> Option<Task<Message>> {
        if !self.panel_open(SidePanel::Explorer) || !self.explorer_follow {
            return None;
        }
        let cwd = self.focused_cwd()?;
        if self.explorer_synced.as_ref() == Some(&cwd) {
            return None;
        }
        self.explorer_synced = Some(cwd.clone());
        if self.explorer.root() == cwd {
            return None;
        }
        Some(load_dir(self.explorer.set_root(cwd)))
    }

    /// The explorer takes the keyboard (the viewer and docked studio let go).
    pub(super) fn focus_explorer(&mut self) {
        self.explorer_focused = true;
        self.viewer.focused = false;
        self.set_prompt_focused(false);
    }

    /// Scrolls the explorer list so the selected row is visible.
    pub(super) fn reveal_explorer_selection(&self) -> Task<Message> {
        const ROW: f32 = 22.0;
        let Some(index) = self.explorer.selected_row() else {
            return Task::none();
        };
        let (offset, height) = self.explorer_scroll;
        let top = index as f32 * ROW;
        let y = if top < offset {
            top
        } else if height > ROW && top + ROW > offset + height {
            top + ROW - height
        } else {
            return Task::none();
        };
        iced::widget::operation::scroll_to(
            EXPLORER_LIST_ID,
            iced::widget::operation::AbsoluteOffset {
                x: None,
                y: Some(y),
            },
        )
    }

    pub(super) fn panel_open(&self, panel: SidePanel) -> bool {
        self.side == Some(panel)
    }

    /// Shows `panel` (replacing any other) or hides it; returns whether it is shown.
    pub(super) fn toggle_panel(&mut self, panel: SidePanel) -> bool {
        self.side = (!self.panel_open(panel)).then_some(panel);
        self.side.is_some()
    }
}
