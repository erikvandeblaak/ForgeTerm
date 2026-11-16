//! Terminal panes: creating, closing, focus, input, find and clipboard.

use super::*;

impl App {
    /// Creates a terminal in `cwd` (default: the directory ForgeTerm started in).
    /// Its shell starts on the first layout, at the pane's real size.
    pub(super) fn spawn_terminal(&mut self, shell: ShellSpec, cwd: Option<PathBuf>) -> TermId {
        self.spawn_terminal_for(shell, cwd, None)
    }

    /// Like [`Self::spawn_terminal`]; agents run without shell integration.
    pub(super) fn spawn_terminal_for(
        &mut self,
        shell: ShellSpec,
        cwd: Option<PathBuf>,
        agent: Option<AgentKind>,
    ) -> TermId {
        let id = self.next_id;
        self.next_id += 1;
        let cwd = Some(cwd.unwrap_or_else(|| self.start_dir.clone()));
        let options = SessionOptions {
            scrollback: self.settings.terminal.scrollback,
            shell_integration: agent.is_none() && self.settings.terminal.shell_integration,
        };
        let mut pane = TerminalPane::new(id, shell, cwd, options, &self.palette);
        pane.agent = agent;
        self.terminals.insert(id, pane);
        id
    }

    pub(super) fn shell_at(&self, index: Option<usize>) -> Option<ShellSpec> {
        match index {
            Some(i) => self.shells.get(i).cloned(),
            None => self
                .default_shell_index()
                .and_then(|i| self.shells.get(i).cloned()),
        }
    }

    /// The configured default shell, or the first detected one.
    pub(super) fn default_shell_index(&self) -> Option<usize> {
        let wanted = self.settings.terminal.default_shell.as_deref();
        wanted
            .and_then(|name| {
                self.shells
                    .iter()
                    .position(|s| s.name.eq_ignore_ascii_case(name))
            })
            .or_else(|| (!self.shells.is_empty()).then_some(0))
    }

    pub(super) fn new_tab(
        &mut self,
        shell_index: Option<usize>,
        cwd: Option<PathBuf>,
    ) -> Task<Message> {
        let Some(shell) = self.shell_at(shell_index) else {
            self.error = Some("No supported shell was found on this system.".into());
            return Task::none();
        };
        let cwd = cwd.or_else(|| self.focused_cwd());
        let id = self.spawn_terminal(shell, cwd);
        self.workspace.push_tab(id);
        Task::none()
    }

    pub(super) fn split(&mut self, axis: Axis) -> Task<Message> {
        // A split starts the same shell as the pane it splits.
        let shell = self
            .workspace
            .focused()
            .and_then(|id| self.terminals.get(&id))
            .filter(|t| t.agent.is_none())
            .map(|t| t.shell.clone())
            .or_else(|| self.shell_at(None));
        let Some(shell) = shell else {
            return Task::none();
        };
        self.restore_zoom();
        let cwd = self.focused_cwd();
        let id = self.spawn_terminal(shell, cwd);
        let split = self
            .workspace
            .active_tab_mut()
            .and_then(|tab| tab.split(axis, id));
        if split.is_some() {
            self.refresh_visible();
            Task::none()
        } else {
            self.terminals.remove(&id);
            Task::none()
        }
    }

    /// Closes a terminal's pane (and its tab if it was the last pane).
    /// Exits the app when nothing is left.
    pub(super) fn close_terminal(&mut self, id: TermId) -> Task<Message> {
        if let Some((tab_index, _)) = self.workspace.locate(id) {
            self.workspace.tabs[tab_index].panes.restore();
        }
        self.workspace.remove_terminal(id);
        self.note_ended(id);
        self.terminals.remove(&id);
        if self.pending_paste.as_ref().is_some_and(|(t, _)| *t == id) {
            self.pending_paste = None;
        }
        self.after_layout_change()
    }

    pub(super) fn close_tab(&mut self, index: usize) -> Task<Message> {
        for id in self.workspace.remove_tab(index) {
            self.note_ended(id);
            self.terminals.remove(&id);
        }
        self.after_layout_change()
    }

    pub(super) fn after_layout_change(&mut self) -> Task<Message> {
        if self.workspace.tabs.is_empty() {
            // Last tab closed: the next start begins fresh, not with that tab.
            self.save_session();
            return iced::exit();
        }
        self.refresh_visible();
        Task::none()
    }

    /// Brings terminals of the active tab up to date and forces a redraw, e.g. after a
    /// tab switch or focus change (focus changes the cursor style).
    pub(super) fn refresh_visible(&mut self) {
        let Some(tab) = self.workspace.active_tab() else {
            return;
        };
        for id in tab.terminals() {
            if let Some(t) = self.terminals.get_mut(&id) {
                t.refresh_if_dirty(&self.palette);
                t.cache.clear();
            }
        }
    }

    pub(super) fn restore_zoom(&mut self) {
        if let Some(tab) = self.workspace.active_tab_mut() {
            tab.panes.restore();
        }
    }

    pub(super) fn focused(&self) -> Option<&TerminalPane> {
        self.workspace
            .focused()
            .and_then(|id| self.terminals.get(&id))
    }

    /// Directory of the focused terminal (see [`TerminalPane::directory`]).
    pub(super) fn focused_cwd(&self) -> Option<PathBuf> {
        self.focused()
            .and_then(TerminalPane::directory)
            .map(Path::to_path_buf)
    }

    pub(super) fn focus_terminal(&mut self, id: TermId) {
        self.viewer.focused = false;
        self.explorer_focused = false;
        self.set_prompt_focused(false);
        if self.workspace.focused() == Some(id) {
            return;
        }
        if let Some((tab_index, pane)) = self.workspace.locate(id) {
            self.workspace.select(tab_index);
            if let Some(tab) = self.workspace.active_tab_mut() {
                tab.focus = pane;
            }
            self.refresh_visible();
        }
    }

    /// Scrolls the tab strip so the active tab is visible after it changed.
    /// Approximate (tabs differ in width): first tab = start, last = end.
    pub(super) fn reveal_active_tab(&mut self) -> Option<Task<Message>> {
        let key = (self.workspace.active, self.workspace.tabs.len());
        if self.revealed_tab == key {
            return None;
        }
        self.revealed_tab = key;
        let x = if key.1 <= 1 {
            0.0
        } else {
            key.0 as f32 / (key.1 - 1) as f32
        };
        Some(iced::widget::operation::snap_to(
            TABS_ID,
            iced::widget::operation::RelativeOffset { x, y: 0.0 },
        ))
    }

    pub(super) fn on_term_msg(&mut self, id: TermId, msg: TermMsg) -> Task<Message> {
        if msg.focuses() {
            self.focus_terminal(id);
        }
        let (metrics, palette) = (self.metrics, &self.palette);
        let Some(t) = self.terminals.get_mut(&id) else {
            return Task::none();
        };
        match msg {
            TermMsg::GridResized(cols, rows) => return t.resize(cols, rows, metrics, palette),
            TermMsg::Scroll(lines) => {
                if let Some(s) = t.session() {
                    s.scroll(lines);
                }
                t.refresh(palette);
            }
            TermMsg::SelectStart(c, r, right) => {
                if let Some(s) = t.session() {
                    s.start_selection(c, r, right);
                }
                t.refresh(palette);
            }
            TermMsg::SelectUpdate(c, r, right) => {
                if let Some(s) = t.session() {
                    s.update_selection(c, r, right);
                }
                t.refresh(palette);
            }
            TermMsg::SelectEnd => {}
            TermMsg::Input { bytes, .. } => {
                if let Some(s) = t.session() {
                    s.write(bytes);
                }
            }
            TermMsg::Paste => return self.run_command(CommandId::Paste),
            TermMsg::OpenUrl(url) => {
                if let Err(e) = open_url(&url) {
                    self.notice = Some(e);
                }
            }
            TermMsg::OpenFile { path, line } => {
                return resolve_file_ref(path, line, t.directory().map(Path::to_path_buf));
            }
        }
        Task::none()
    }

    pub(super) fn set_font_size(&mut self, size: f32) {
        self.metrics = Metrics::new(size.clamp(8.0, 32.0));
        for t in self.terminals.values() {
            t.cache.clear();
        }
    }

    /// Inserts text at the focused terminal's prompt like a paste: never followed by
    /// Enter, and multi-line text without bracketed paste asks for confirmation.
    pub(super) fn insert_text(&mut self, text: String) {
        if let Some(id) = self.workspace.focused() {
            self.insert_text_into(id, text);
        }
    }

    pub(super) fn insert_text_into(&mut self, id: TermId, text: String) {
        let Some(t) = self.terminals.get(&id) else {
            return;
        };
        if paste_needs_confirmation(&text, t.modes.bracketed_paste) {
            self.pending_paste = Some((id, text));
        } else if let Some(s) = t.session() {
            s.paste(&text);
        }
    }

    /// The first search for the typed query: from the cursor upwards, most recent
    /// match first.
    pub(super) fn term_find_first(&mut self) {
        let Some(query) = self.term_find.as_ref().map(|f| f.query.clone()) else {
            return;
        };
        if let Some(s) = self.focused().and_then(TerminalPane::session) {
            s.clear_selection();
        }
        let outcome = self.term_search(&query, true);
        if let Some(f) = &mut self.term_find {
            f.outcome = outcome;
            f.searched = true;
        }
    }

    /// Searches the focused terminal and refreshes its view.
    pub(super) fn term_search(
        &mut self,
        query: &str,
        older: bool,
    ) -> Option<forgeterm_terminal::SearchOutcome> {
        let id = self.workspace.focused()?;
        let t = self.terminals.get_mut(&id)?;
        if query.is_empty() {
            return None;
        }
        let outcome = t.session()?.search(query, older);
        t.refresh(&self.palette);
        Some(outcome)
    }

    /// A program in a terminal set the clipboard (OSC 52). Allowed, as in other
    /// terminals, but never silently: the user sees which tab did it. Huge
    /// payloads are refused.
    pub(super) fn program_clipboard(&mut self, id: TermId, text: String) -> Task<Message> {
        const MAX: usize = 1024 * 1024;
        let tab = self.workspace.locate(id).map_or(0, |(i, _)| i + 1);
        let who = self.terminals.get(&id).map_or_else(
            || "A program".to_owned(),
            |t| format!("{} (tab {tab})", t.display_title()),
        );
        if text.len() > MAX {
            self.notice = Some(format!(
                "{who} tried to put {} KB on the clipboard; refused (limit 1 MB).",
                text.len() / 1024
            ));
            return Task::none();
        }
        self.notice = Some(format!(
            "{who} copied {} character{} to the clipboard.",
            text.chars().count(),
            if text.chars().count() == 1 { "" } else { "s" }
        ));
        clipboard::write(text)
    }

    pub(super) fn copy_selection(&mut self) -> Task<Message> {
        let Some(id) = self.workspace.focused() else {
            return Task::none();
        };
        let Some(t) = self.terminals.get_mut(&id) else {
            return Task::none();
        };
        let Some(selected) = t.session().and_then(|s| s.selection_text()) else {
            return Task::none();
        };
        if let Some(s) = t.session() {
            s.clear_selection();
        }
        t.refresh(&self.palette);
        let selected = if cfg!(windows) {
            selected.replace('\n', "\r\n")
        } else {
            selected
        };
        clipboard::write(selected)
    }
}
