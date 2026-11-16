//! Keyboard routing: dialogs, overlays, shortcuts, then the terminal.

use super::*;

impl App {
    pub(super) fn on_key(
        &mut self,
        key: keyboard::Key,
        text: Option<String>,
        mods: keyboard::Modifiers,
        captured: bool,
    ) -> Task<Message> {
        // A close confirmation answers Enter / Esc and swallows everything else.
        if self.pending_close.is_some() {
            return match key {
                keyboard::Key::Named(Named::Enter) => self.update(Message::ConfirmClose),
                keyboard::Key::Named(Named::Escape) => self.update(Message::CancelClose),
                _ => Task::none(),
            };
        }
        // While a paste confirmation is open, the keyboard answers it and nothing else.
        if self.pending_paste.is_some() {
            return match key {
                keyboard::Key::Named(Named::Enter) => self.update(Message::ConfirmPaste),
                keyboard::Key::Named(Named::Escape) => self.update(Message::CancelPaste),
                _ => Task::none(),
            };
        }

        // The worktree dialog: Esc cancels; Enter submits through the branch field
        // (or here when the field does not have the keyboard); nothing else leaks.
        if self.worktree_form.is_some() {
            return match key {
                keyboard::Key::Named(Named::Escape) => self.update(Message::WorktreeFormCancel),
                keyboard::Key::Named(Named::Enter) if !captured => {
                    self.update(Message::WorktreeFormSubmit)
                }
                _ => Task::none(),
            };
        }
        if self.pending_worktree_remove.is_some() {
            return match key {
                keyboard::Key::Named(Named::Enter) => self.update(Message::WorktreeRemoveConfirm),
                keyboard::Key::Named(Named::Escape) => self.update(Message::WorktreeRemoveCancel),
                _ => Task::none(),
            };
        }
        // Modal overlays own the keyboard while open. A captured Enter is the one
        // that opened them from the command palette's input: it must not close them.
        let closes = |key: &keyboard::Key| match key {
            keyboard::Key::Named(Named::Escape) => true,
            keyboard::Key::Named(Named::Enter) => !captured,
            _ => false,
        };
        if self.shortcuts_open {
            if closes(&key) {
                self.shortcuts_open = false;
            }
            return Task::none();
        }
        if self.about_open {
            if closes(&key) {
                self.about_open = false;
            }
            return Task::none();
        }
        if self.menu.is_some() {
            return self.menu_key(&key);
        }
        if self.command_palette.is_some() {
            return self.palette_key(&key, text.as_deref(), mods, captured);
        }
        if self.prompt_studio.is_some() && !self.prompt_docked {
            return self.prompt_studio_key(&key, mods, captured);
        }
        if self.git.diff.is_some() {
            if matches!(key, keyboard::Key::Named(Named::Escape)) {
                self.git.diff = None;
            }
            return Task::none();
        }
        if self.git.history.is_some() {
            if matches!(key, keyboard::Key::Named(Named::Escape)) {
                self.git.history = None;
            }
            return Task::none();
        }
        // Terminal search bar: Esc closes, F3 / Shift+F3 step, also from its input.
        if self.term_find.is_some() && !self.viewer.focused {
            match key {
                keyboard::Key::Named(Named::Escape) => {
                    self.term_find = None;
                    return Task::none();
                }
                keyboard::Key::Named(Named::F3) => {
                    return self.update(Message::TermFindStep(!mods.shift()));
                }
                // The match is the terminal selection: Ctrl+C copies it, even while
                // the search field (which would swallow the key) has the focus.
                keyboard::Key::Character(c)
                    if mods.control()
                        && !mods.shift()
                        && !mods.alt()
                        && c.eq_ignore_ascii_case("c")
                        && self
                            .focused()
                            .and_then(TerminalPane::session)
                            .is_some_and(|s| s.selection_text().is_some()) =>
                {
                    return self.copy_selection();
                }
                _ => {}
            }
        }
        // The viewer's find bar closes with Esc, also while its input has focus.
        if self.viewer.visible
            && self.viewer.find_open
            && matches!(key, keyboard::Key::Named(Named::Escape))
            && (self.viewer.focused || captured)
        {
            return self.viewer_msg(ViewerMsg::CloseFind);
        }
        // Docked studio with the keyboard: Esc gives it back to the terminal (the field
        // also lets go of it), Tab moves between fields.
        let studio_keys = self.prompt_docked && self.prompt_focused && self.prompt_studio.is_some();
        if studio_keys {
            if matches!(key, keyboard::Key::Named(Named::Escape)) {
                self.set_prompt_focused(false);
                return unfocus_widgets();
            }
            if !captured && matches!(key, keyboard::Key::Named(Named::Tab)) {
                return if mods.shift() {
                    iced::widget::operation::focus_previous()
                } else {
                    iced::widget::operation::focus_next()
                };
            }
        }
        // The "Go to Folder" field closes with Esc, also while it has the keyboard.
        if self.explorer_goto.is_some() && matches!(key, keyboard::Key::Named(Named::Escape)) {
            self.explorer_goto = None;
            return unfocus_widgets();
        }
        // Keys handled by a widget never reach the terminal.
        if captured {
            return Task::none();
        }
        // Keys the studio's fields do not use (F-keys, Ctrl+letter, …) never leak to
        // the terminal either; app shortcuts still work.
        if studio_keys {
            return self.app_shortcut(&key, mods).unwrap_or_else(Task::none);
        }
        // The focused viewer takes navigation keys; app shortcuts still work, the
        // terminal gets nothing.
        if self.viewer.visible && self.viewer.focused {
            if let Some(msg) = viewer_key(&key, mods, &self.viewer) {
                return self.viewer_msg(msg);
            }
            return self.app_shortcut(&key, mods).unwrap_or_else(Task::none);
        }

        // The focused explorer takes navigation keys (Esc gives the keyboard back
        // to the terminal); app shortcuts still work; other keys go to the terminal.
        if self.panel_open(SidePanel::Explorer) && self.explorer_focused {
            if matches!(key, keyboard::Key::Named(Named::Escape)) {
                self.explorer_focused = false;
                return Task::none();
            }
            if let Some(nav) = explorer_key(&key, mods) {
                let page = ((self.explorer_scroll.1 / 22.0) as usize).max(1);
                let task = match self.explorer.navigate(nav, page) {
                    forgeterm_explorer::NavOutcome::None => Task::none(),
                    forgeterm_explorer::NavOutcome::Load(dir) => load_dir(dir),
                    // Like an editor: the opened file gets the keyboard (Esc there
                    // returns it to the terminal).
                    forgeterm_explorer::NavOutcome::Open(file) => {
                        self.viewer_msg(ViewerMsg::Open(file))
                    }
                };
                return Task::batch([task, self.reveal_explorer_selection()]);
            }
            if let Some(task) = self.app_shortcut(&key, mods) {
                return task;
            }
            // A modifier on its own (Shift before a capital, Ctrl before a shortcut,
            // Alt) is not typing yet: the explorer keeps the keyboard.
            if matches!(
                key,
                keyboard::Key::Named(
                    Named::Shift | Named::Control | Named::Alt | Named::Super | Named::AltGraph
                )
            ) {
                return Task::none();
            }
            // Any other key is typing meant for the terminal: it gets the keyboard
            // back, including this key, so nothing typed is lost.
            self.explorer_focused = false;
        }

        if let Some(task) = self.app_shortcut(&key, mods) {
            return task;
        }
        if self.mission.visible {
            return self.mission_key(&key, mods);
        }
        // The Git Control Center has no keyboard actions of its own besides Esc;
        // nothing reaches the terminals hidden behind it.
        if self.git_dash.visible {
            if matches!(key, keyboard::Key::Named(Named::Escape)) {
                return self.run_command(CommandId::ToggleGitDashboard);
            }
            return Task::none();
        }
        // Compare has text fields; Esc closes it and nothing reaches the terminals.
        if self.compare.visible {
            if matches!(key, keyboard::Key::Named(Named::Escape)) {
                return self.run_command(CommandId::ToggleCompare);
            }
            return Task::none();
        }

        let Some(focused) = self.workspace.focused() else {
            return Task::none();
        };
        let Some(t) = self.terminals.get_mut(&focused) else {
            return Task::none();
        };

        // A pane whose shell ended: Enter starts it again.
        if !t.is_running() {
            // Not started yet (first layout pending): nothing to restart.
            if matches!(key, keyboard::Key::Named(Named::Enter)) && !t.is_awaiting_size() {
                return t.start(self.metrics, &self.palette);
            }
            return Task::none();
        }

        if let Some((tkey, tmods)) = keys::translate(&key, text.as_deref(), mods)
            && let Some(bytes) = forgeterm_terminal::encode_key_kitty(
                &tkey,
                tmods,
                t.modes.app_cursor,
                t.modes.keyboard,
            )
            && let Some(session) = t.session()
        {
            session.input(bytes);
        }
        Task::none()
    }

    pub(super) fn palette_key(
        &mut self,
        key: &keyboard::Key,
        text: Option<&str>,
        mods: keyboard::Modifiers,
        captured: bool,
    ) -> Task<Message> {
        let toggle = keys::binding(key, mods)
            .and_then(|b| self.keymap.lookup(b))
            .is_some_and(|id| id == CommandId::CommandPalette);
        // Escape always closes, even though the text input also reacts to it.
        if matches!(key, keyboard::Key::Named(Named::Escape)) || toggle {
            self.command_palette = None;
            return Task::none();
        }
        // The focused text input handles typing, editing and Enter itself.
        if captured {
            return Task::none();
        }
        let Some(p) = &mut self.command_palette else {
            return Task::none();
        };
        match key {
            keyboard::Key::Named(Named::ArrowDown) => p.move_selection(1),
            keyboard::Key::Named(Named::ArrowUp) => p.move_selection(-1),
            keyboard::Key::Named(Named::PageDown) => p.move_selection(8),
            keyboard::Key::Named(Named::PageUp) => p.move_selection(-8),
            keyboard::Key::Named(Named::Tab) if !mods.control() => {
                p.move_selection(if mods.shift() { -1 } else { 1 });
            }
            // Keys that arrive before the input has focus (typing right after
            // Ctrl+Shift+P) are not lost: Enter runs, text extends the query.
            keyboard::Key::Named(Named::Enter) => {
                return self.update(Message::PaletteRun(None));
            }
            _ => {
                if !mods.control()
                    && !mods.alt()
                    && let Some(t) = text.filter(|t| !t.chars().any(char::is_control))
                {
                    p.push_query(t);
                }
            }
        }
        Task::none()
    }

    /// The studio owns the keyboard: its editors take typing (including Ctrl+C/V/X),
    /// Tab moves between fields, Esc closes. Nothing reaches the terminal.
    pub(super) fn prompt_studio_key(
        &mut self,
        key: &keyboard::Key,
        mods: keyboard::Modifiers,
        captured: bool,
    ) -> Task<Message> {
        let toggle = keys::binding(key, mods)
            .and_then(|b| self.keymap.lookup(b))
            .is_some_and(|id| id == CommandId::OpenPromptStudio);
        if matches!(key, keyboard::Key::Named(Named::Escape)) || (toggle && !captured) {
            self.prompt_studio = None;
            return Task::none();
        }
        if !captured && matches!(key, keyboard::Key::Named(Named::Tab)) {
            return if mods.shift() {
                iced::widget::operation::focus_previous()
            } else {
                iced::widget::operation::focus_next()
            };
        }
        Task::none()
    }

    pub(super) fn menu_key(&mut self, key: &keyboard::Key) -> Task<Message> {
        let Some(mut state) = self.menu else {
            return Task::none();
        };
        match key {
            keyboard::Key::Named(Named::Escape) => {
                self.menu = None;
                return Task::none();
            }
            keyboard::Key::Named(Named::ArrowLeft) => state.switch(self.menus.len(), -1),
            keyboard::Key::Named(Named::ArrowRight) => state.switch(self.menus.len(), 1),
            keyboard::Key::Named(Named::ArrowDown) => state.step(&self.menu_rows(state.open), 1),
            keyboard::Key::Named(Named::ArrowUp) => state.step(&self.menu_rows(state.open), -1),
            keyboard::Key::Named(Named::Enter | Named::Space) => {
                let command = state.highlighted_command(&self.menu_rows(state.open));
                if let Some(id) = command {
                    return self.run_command(id);
                }
            }
            // Everything else is swallowed: an open menu never types into the terminal.
            _ => {}
        }
        self.menu = Some(state);
        Task::none()
    }

    /// Application-level shortcuts. Returns `None` when the key belongs to the terminal.
    pub(super) fn app_shortcut(
        &mut self,
        key: &keyboard::Key,
        mods: keyboard::Modifiers,
    ) -> Option<Task<Message>> {
        // Ctrl+C with an active selection copies instead of sending SIGINT. Conditional,
        // so it lives here rather than in the keymap.
        if mods.control()
            && !mods.shift()
            && !mods.alt()
            && matches!(key, keyboard::Key::Character(c) if c.eq_ignore_ascii_case("c"))
            && self
                .focused()
                .and_then(TerminalPane::session)
                .is_some_and(|s| s.selection_text().is_some())
        {
            return Some(self.copy_selection());
        }

        let id = self.keymap.lookup(keys::binding(key, mods)?)?;
        // Pane focus keys only make sense with several panes; otherwise Alt+Arrow
        // belongs to the program in the terminal.
        if matches!(id, CommandId::FocusPane(_)) && !self.has_splits() {
            return None;
        }
        // A shortcut follows the same rule as the menu and the palette: a command
        // that does not apply now does nothing. The key is still ForgeTerm's, so
        // it does not fall through to the program either.
        if !self.is_applicable(id) {
            return Some(Task::none());
        }
        Some(self.run_command(id))
    }

    pub(super) fn has_splits(&self) -> bool {
        self.workspace
            .active_tab()
            .is_some_and(|tab| tab.panes.len() > 1)
    }

    /// Esc closes, arrows / Tab move the selection, Enter opens the terminal.
    /// Other keys are swallowed: the terminals are hidden behind the dashboard.
    pub(super) fn mission_key(
        &mut self,
        key: &keyboard::Key,
        mods: keyboard::Modifiers,
    ) -> Task<Message> {
        let order = self.mission_order();
        let pos = self
            .mission
            .selected
            .and_then(|id| order.iter().position(|&o| o == id));
        let step = |forward: bool| -> Option<TermId> {
            if order.is_empty() {
                return None;
            }
            let i = match (pos, forward) {
                (None, _) => 0,
                (Some(i), true) => (i + 1) % order.len(),
                (Some(i), false) => (i + order.len() - 1) % order.len(),
            };
            Some(order[i])
        };
        match key {
            keyboard::Key::Named(Named::Escape) => return self.toggle_mission(),
            keyboard::Key::Named(Named::ArrowRight | Named::ArrowDown) => {
                self.mission.selected = step(true);
            }
            keyboard::Key::Named(Named::ArrowLeft | Named::ArrowUp) => {
                self.mission.selected = step(false);
            }
            keyboard::Key::Named(Named::Tab) => self.mission.selected = step(!mods.shift()),
            keyboard::Key::Named(Named::Enter) => {
                if let Some(id) = self.mission.selected.filter(|id| order.contains(id)) {
                    return self.run_command(CommandId::GoToTerminal(id));
                }
            }
            _ => {}
        }
        Task::none()
    }
}

/// Keys of the focused viewer; `None` lets the key fall through to app shortcuts.
pub(super) fn viewer_key(
    key: &keyboard::Key,
    mods: keyboard::Modifiers,
    v: &Viewer,
) -> Option<ViewerMsg> {
    let rows = v.tab().map_or(30, |t| t.rows) as i32;
    let ctrl = mods.control() && !mods.alt();
    Some(match key {
        keyboard::Key::Named(Named::ArrowUp) => ViewerMsg::Scroll(1),
        keyboard::Key::Named(Named::ArrowDown) => ViewerMsg::Scroll(-1),
        keyboard::Key::Named(Named::PageUp) => ViewerMsg::Scroll(rows - 1),
        keyboard::Key::Named(Named::PageDown) => ViewerMsg::Scroll(-(rows - 1)),
        keyboard::Key::Named(Named::Home) => ViewerMsg::Scroll(i32::MAX / 2),
        keyboard::Key::Named(Named::End) => ViewerMsg::Scroll(-(i32::MAX / 2)),
        keyboard::Key::Named(Named::F3) if mods.shift() => ViewerMsg::FindPrev,
        keyboard::Key::Named(Named::F3) => ViewerMsg::FindNext,
        keyboard::Key::Named(Named::Escape) => ViewerMsg::Escape,
        keyboard::Key::Character(c) if ctrl && !mods.shift() && c.eq_ignore_ascii_case("c") => {
            ViewerMsg::CopySelection
        }
        keyboard::Key::Character(c) if ctrl && !mods.shift() && c.eq_ignore_ascii_case("f") => {
            ViewerMsg::OpenFind
        }
        keyboard::Key::Character(c) if ctrl && !mods.shift() && c.eq_ignore_ascii_case("a") => {
            ViewerMsg::SelectAll
        }
        _ => return None,
    })
}

/// Navigation keys of the focused explorer (no modifiers except Shift-less).
pub(super) fn explorer_key(
    key: &keyboard::Key,
    mods: keyboard::Modifiers,
) -> Option<forgeterm_explorer::Nav> {
    use forgeterm_explorer::Nav;
    if mods.control() || mods.alt() || mods.shift() {
        return None;
    }
    let keyboard::Key::Named(named) = key else {
        return None;
    };
    Some(match named {
        Named::ArrowUp => Nav::Up,
        Named::ArrowDown => Nav::Down,
        Named::ArrowLeft => Nav::Left,
        Named::ArrowRight => Nav::Right,
        Named::PageUp => Nav::PageUp,
        Named::PageDown => Nav::PageDown,
        Named::Home => Nav::Home,
        Named::End => Nav::End,
        Named::Enter => Nav::Activate,
        _ => return None,
    })
}
