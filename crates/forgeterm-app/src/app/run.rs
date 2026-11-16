//! Commands: the single place they run, and when they apply.

use super::*;

impl App {
    /// The single place where commands are executed.
    pub fn run_command(&mut self, id: CommandId) -> Task<Message> {
        self.menu = None;
        if self.main_covered() && id.shows_terminals() {
            self.mission.visible = false;
            self.git_dash.visible = false;
            self.compare.visible = false;
            self.refresh_visible();
        }
        if id.is_recordable() {
            self.recent_commands.retain(|c| *c != id);
            self.recent_commands.insert(0, id);
            self.recent_commands.truncate(command_palette::RECENT_LIMIT);
        }
        match id {
            CommandId::CommandPalette => {
                if self.command_palette.take().is_some() {
                    return Task::none();
                }
                let entries = self.palette_entries();
                self.command_palette = Some(CommandPalette::new(entries, &self.recent_commands));
                return iced::widget::operation::focus(command_palette::INPUT_ID);
            }
            CommandId::NewTab => return self.new_tab(None, None),
            CommandId::NewSshTab(i) => return self.new_ssh_tab(i),
            CommandId::NewTabWithShell(i) => {
                if i < self.shells.len() {
                    return self.new_tab(Some(i), None);
                }
            }
            CommandId::CloseTab => {
                let ids = self
                    .workspace
                    .active_tab()
                    .map(|t| t.terminals().collect())
                    .unwrap_or_default();
                return self.request_close(CloseTarget::Tab(ids));
            }
            CommandId::ClosePane => {
                if let Some(id) = self.workspace.focused() {
                    return self.request_close(CloseTarget::Pane(id));
                }
            }
            CommandId::NextTab | CommandId::PreviousTab => {
                self.workspace.cycle(id == CommandId::NextTab);
                self.refresh_visible();
            }
            CommandId::SelectTab(i) => {
                if self.workspace.select(i) {
                    self.refresh_visible();
                }
            }
            CommandId::SplitRight => return self.split(Axis::Vertical),
            CommandId::SplitDown => return self.split(Axis::Horizontal),
            CommandId::TogglePaneZoom => {
                if let Some(tab) = self.workspace.active_tab_mut()
                    && tab.panes.len() > 1
                {
                    if tab.panes.maximized().is_some() {
                        tab.panes.restore();
                    } else {
                        tab.panes.maximize(tab.focus);
                    }
                    self.refresh_visible();
                }
            }
            CommandId::FocusPane(dir) => {
                let direction = match dir {
                    Dir::Left => Direction::Left,
                    Dir::Right => Direction::Right,
                    Dir::Up => Direction::Up,
                    Dir::Down => Direction::Down,
                };
                self.restore_zoom();
                if let Some(tab) = self.workspace.active_tab_mut()
                    && tab.focus_adjacent(direction)
                {
                    self.refresh_visible();
                }
            }
            CommandId::RestartShell => {
                let (metrics, palette) = (self.metrics, &self.palette);
                if let Some(focused) = self.workspace.focused()
                    && let Some(t) = self.terminals.get_mut(&focused)
                {
                    return t.restart(metrics, palette);
                }
            }
            CommandId::ClearScrollback
            | CommandId::ScrollToTop
            | CommandId::ScrollToBottom
            | CommandId::ScrollPageUp
            | CommandId::ScrollPageDown => {
                let palette = &self.palette;
                if let Some(focused) = self.workspace.focused()
                    && let Some(t) = self.terminals.get_mut(&focused)
                {
                    if let Some(s) = t.session() {
                        let page = i32::from(t.grid.1.saturating_sub(1).max(1));
                        match id {
                            CommandId::ClearScrollback => s.clear_scrollback(),
                            CommandId::ScrollToTop => s.scroll_to_top(),
                            CommandId::ScrollToBottom => s.scroll_to_bottom(),
                            CommandId::ScrollPageUp => s.scroll(page),
                            _ => s.scroll(-page),
                        }
                    }
                    t.refresh(palette);
                }
            }
            CommandId::PreviousCommand | CommandId::NextCommand => {
                let palette = &self.palette;
                if let Some(focused) = self.workspace.focused()
                    && let Some(t) = self.terminals.get_mut(&focused)
                {
                    if let Some(s) = t.session() {
                        s.jump_to_prompt(id == CommandId::PreviousCommand);
                    }
                    t.refresh(palette);
                }
            }
            CommandId::CopyLastCommandOutput => {
                let output = self
                    .focused()
                    .and_then(TerminalPane::session)
                    .map(forgeterm_terminal::Session::last_command_output);
                match output {
                    Some(Ok(out)) => return clipboard::write(out.text),
                    Some(Err(e)) => self.notice = Some(format!("Nothing copied: {e}.")),
                    None => {}
                }
            }
            CommandId::InsertLastOutputIntoAgent => return self.insert_last_output_into_agent(),
            CommandId::Copy => return self.copy_selection(),
            CommandId::FindInTerminal => {
                if self.term_find.is_none() {
                    self.term_find = Some(TermFind::default());
                }
                return iced::widget::operation::focus(TERM_FIND_ID);
            }
            CommandId::Paste => return Task::done(Message::Paste),
            CommandId::ZoomIn => self.set_font_size(self.metrics.font_size + 1.0),
            CommandId::ZoomOut => self.set_font_size(self.metrics.font_size - 1.0),
            CommandId::ZoomReset => self.set_font_size(self.settings.terminal.font_size),
            CommandId::OpenSettings => {
                if let Some(path) = self.settings_path.clone() {
                    let opened = forgeterm_settings::ensure_file(&path)
                        .map_err(|e| format!("could not create {}: {e}", path.display()))
                        .and_then(|()| open_in_editor(&path));
                    if let Err(e) = opened {
                        self.settings_problems = vec![e];
                    }
                }
            }
            CommandId::ReloadSettings => self.load_settings(false),
            CommandId::SetTheme(choice) => self.apply_theme(choice),
            // Like VS Code: hidden → show with the keyboard, shown → take the
            // keyboard, already focused → hide (keyboard back to the terminal).
            CommandId::ToggleExplorer => {
                if self.panel_open(SidePanel::Explorer) && self.explorer_focused {
                    self.side = None;
                    self.explorer_focused = false;
                    self.explorer_goto = None;
                } else {
                    self.side = Some(SidePanel::Explorer);
                    self.focus_explorer();
                    return unfocus_widgets();
                }
            }
            CommandId::ExplorerGoToFolder => {
                self.side = Some(SidePanel::Explorer);
                self.focus_explorer();
                let current = self.explorer.root().to_string_lossy().into_owned();
                self.explorer_goto = Some((current, None));
                return Task::batch([
                    iced::widget::operation::focus(EXPLORER_GOTO_ID),
                    iced::widget::operation::select_all(EXPLORER_GOTO_ID),
                ]);
            }
            CommandId::ToggleAiPanel => {
                self.toggle_panel(SidePanel::Ai);
            }
            CommandId::AiRefresh => {
                self.agents = AgentKind::ALL
                    .iter()
                    .map(|k| (*k, AgentState::Detecting))
                    .collect();
                self.ai_context_for = None;
                return detect_agents();
            }
            CommandId::StartAgent(kind, split) => return self.start_agent(kind, split),
            CommandId::ToggleMissionControl => return self.toggle_mission(),
            CommandId::GoToTerminal(id) => self.go_to_terminal(id),
            CommandId::TerminalFolder(id) => {
                let Some(dir) = self
                    .terminals
                    .get(&id)
                    .and_then(TerminalPane::directory)
                    .filter(|d| d.to_str().is_some_and(forgeterm_terminal::is_local_path))
                    .map(Path::to_path_buf)
                else {
                    return Task::none();
                };
                self.side = Some(SidePanel::Explorer);
                return load_dir(self.explorer.set_root(dir));
            }
            CommandId::TerminalGit(id) => {
                self.go_to_terminal(id);
                self.side = Some(SidePanel::Git);
                return self.refresh_git(true);
            }
            CommandId::StopTerminal(id) => return self.request_close(CloseTarget::Pane(id)),
            CommandId::ToggleMonitor => {
                if self.toggle_panel(SidePanel::Monitor) {
                    return self.sample_monitor().unwrap_or_else(Task::none);
                }
            }
            CommandId::ToggleViewer => {
                self.viewer.visible = !self.viewer.visible;
                if !self.viewer.visible {
                    self.viewer.focused = false;
                }
            }
            CommandId::ViewSelectedFile => {
                if let Some(path) = self
                    .explorer
                    .selected()
                    .filter(|e| !e.is_dir)
                    .map(|e| e.path.clone())
                {
                    return self.viewer_msg(ViewerMsg::Open(path));
                }
            }
            CommandId::ToggleGitPanel => {
                if self.toggle_panel(SidePanel::Git) {
                    return self.refresh_git(true);
                }
            }
            CommandId::GitRefresh => {
                let compare = if self.compare.visible {
                    self.load_compare()
                } else {
                    Task::none()
                };
                return Task::batch([self.refresh_git(true), self.load_git_dash(true), compare]);
            }
            CommandId::CloseTabAt(index) => {
                let ids = self
                    .workspace
                    .tabs
                    .get(index)
                    .map(|t| t.terminals().collect())
                    .unwrap_or_default();
                return self.request_close(CloseTarget::Tab(ids));
            }
            CommandId::InsertPrompt => return self.insert_prompt(),
            CommandId::CopyPrompt => {
                let text = self
                    .prompt_studio
                    .as_ref()
                    .and_then(PromptStudio::rendered)
                    .map(|r| r.text);
                // The floating window closes; the docked panel stays.
                if !self.prompt_docked {
                    self.prompt_studio = None;
                }
                if let Some(text) = text {
                    return clipboard::write(text);
                }
            }
            CommandId::ReloadPrompts => {
                if self.prompt_studio.is_some() {
                    return load_prompts(self.prompts_dir());
                }
            }
            CommandId::GitCommit => {
                if self.git.can_commit() {
                    let message = self.git.commit_message.text();
                    return self.git_action(GitAction::Commit(message));
                }
            }
            CommandId::GitStageAll => {
                let paths = self.git.stage_all_paths();
                if !paths.is_empty() {
                    return self.git_action(GitAction::Stage(paths));
                }
            }
            CommandId::GitUnstageAll => {
                let paths = self.git.unstage_all_paths();
                if !paths.is_empty() {
                    return self.git_action(GitAction::Unstage(paths));
                }
            }
            CommandId::GitTrustRepository => {
                if let Some(Repo::Untrusted { root, .. }) = self.git.repo.as_ref() {
                    self.git.trusted.insert(root.clone());
                    return self.refresh_git(true);
                }
            }
            CommandId::GitToggleRemoteBranches => self.git.show_remotes = !self.git.show_remotes,
            CommandId::ToggleCompare => {
                self.compare.visible = !self.compare.visible;
                if !self.compare.visible {
                    self.refresh_visible();
                    return Task::none();
                }
                self.mission.visible = false;
                self.git_dash.visible = false;
                return Task::batch([self.refresh_git(true), self.load_compare()]);
            }
            CommandId::CompareSwap => {
                if let Some([a, b]) = self.compare.paths() {
                    self.compare.a = Some(b);
                    self.compare.b = Some(a);
                    // The same pair: the file stays selected, its rows swap sides
                    // once the load is in (`CompareLoaded` reads it again).
                    if let Some(d) = &mut self.compare.diff {
                        d.result = None;
                    }
                    return self.load_compare();
                }
            }
            CommandId::CompareOpenTests => return self.open_compare_tests(),
            CommandId::CompareInsertPrompt => self.insert_compare_prompt(),
            CommandId::ToggleGitDashboard => {
                self.git_dash.visible = !self.git_dash.visible;
                if !self.git_dash.visible {
                    self.refresh_visible();
                    return Task::none();
                }
                self.mission.visible = false;
                self.compare.visible = false;
                return Task::batch([self.refresh_git(true), self.load_git_dash(true)]);
            }
            CommandId::NewAgentWorktree => {
                let Some(Repo::Found { root, .. }) = self.git.repo.as_ref() else {
                    return Task::none();
                };
                let Some(kind) = AgentKind::ALL
                    .into_iter()
                    .find(|k| self.found_agent(*k).is_some())
                else {
                    return Task::none();
                };
                let existing = self.branch_names();
                let branch = worktree_form::suggest_branch(kind, &existing);
                self.worktree_form = Some(WorktreeForm {
                    root: root.clone(),
                    kind,
                    branch,
                    existing,
                    error: None,
                    busy: false,
                });
                // The panel's list can be missing or old: read it now.
                let root = root.clone();
                let names = background(
                    move || {
                        forgeterm_git::branches(&root)
                            .map(|l| l.into_iter().map(|b| b.name).collect())
                            .unwrap_or_default()
                    },
                    Message::WorktreeFormBranches,
                );
                return Task::batch([
                    iced::widget::operation::focus(worktree_form::BRANCH_INPUT_ID),
                    iced::widget::operation::select_all(worktree_form::BRANCH_INPUT_ID),
                    names,
                ]);
            }
            CommandId::RemoveWorktree(id) => {
                if let Some(target) = self.removable_worktree(id) {
                    self.pending_worktree_remove = Some(target);
                }
            }
            CommandId::WorktreeTerminal(i) => {
                if let Some(path) = self.worktree_path(i) {
                    return self.new_tab(None, Some(path));
                }
            }
            CommandId::GitShowHistory => {
                if let Some(root) = self.git.root().cloned() {
                    self.git.history = Some(HistoryView::default());
                    let key = root.clone();
                    return background(
                        move || HistoryData {
                            entries: forgeterm_git::history(&root, 300),
                            activity: forgeterm_git::activity(&root, 30),
                            root,
                        },
                        move |data| {
                            debug_assert_eq!(data.root, key);
                            Message::GitHistoryLoaded(Box::new(data))
                        },
                    );
                }
            }
            CommandId::OpenPromptStudio => {
                self.set_prompt_focused(false);
                if self.prompt_studio.take().is_none() {
                    return self.open_prompt_studio();
                }
            }
            CommandId::TogglePromptStudioDock => {
                self.prompt_docked = !self.prompt_docked;
                self.set_prompt_focused(false);
                if self.prompt_studio.is_none() {
                    return self.open_prompt_studio();
                }
            }
            CommandId::OpenPromptsFolder => {
                if let Some(dir) = self.prompts_dir()
                    && let Err(e) = open_prompts_folder(&dir)
                {
                    self.notice = Some(e);
                }
            }
            CommandId::ExplorerToggleFollow => {
                self.explorer_follow = !self.explorer_follow;
                // Turning it on jumps to the shell's directory right away.
                self.explorer_synced = None;
            }
            CommandId::ExplorerToggleHidden => {
                let show = !self.explorer.show_hidden();
                self.explorer.set_show_hidden(show);
            }
            CommandId::ExplorerRefresh => {
                return Task::batch(self.explorer.refresh().into_iter().map(load_dir));
            }
            CommandId::ExplorerParent => {
                if let Some(parent) = self.explorer.parent() {
                    return load_dir(self.explorer.set_root(parent));
                }
            }
            CommandId::ExplorerOpenSelected => {
                if let Some(dir) = self
                    .explorer
                    .selected()
                    .filter(|e| e.is_dir)
                    .map(|e| e.path.clone())
                {
                    return load_dir(self.explorer.set_root(dir));
                }
            }
            CommandId::InsertSelectedPath => {
                if let Some(path) = self.explorer.selected().map(|e| e.path.clone()) {
                    self.insert_paths(&[path]);
                }
            }
            CommandId::CopySelectedPath => {
                if let Some(e) = self.explorer.selected() {
                    return clipboard::write(e.path.to_string_lossy().into_owned());
                }
            }
            CommandId::NewTabInSelectedFolder => {
                let dir = self.explorer.selected().map(|e| {
                    if e.is_dir {
                        e.path.clone()
                    } else {
                        e.path
                            .parent()
                            .map_or_else(|| e.path.clone(), Path::to_path_buf)
                    }
                });
                if let Some(dir) = dir {
                    return self.new_tab(None, Some(dir));
                }
            }
            CommandId::About => self.about_open = true,
            CommandId::Website => {
                if let Err(e) = open_url(crate::commands::WEBSITE) {
                    self.notice = Some(e);
                }
            }
            CommandId::KeyboardShortcuts => self.shortcuts_open = true,
            // Same as closing the window: sessions are dropped, shells are shut down.
            CommandId::Quit => return self.request_close(CloseTarget::Quit),
        }
        Task::none()
    }

    /// Whether a command can do something right now. The palette only lists
    /// applicable commands, so nothing it shows is a no-op.
    pub(super) fn is_applicable(&self, id: CommandId) -> bool {
        let focused = self.focused();
        match id {
            CommandId::FocusPane(_) | CommandId::TogglePaneZoom => self.has_splits(),
            CommandId::NextTab | CommandId::PreviousTab => self.workspace.tabs.len() > 1,
            CommandId::PreviousCommand
            | CommandId::NextCommand
            | CommandId::CopyLastCommandOutput => focused
                .and_then(TerminalPane::session)
                .is_some_and(forgeterm_terminal::Session::has_prompt_marks),
            CommandId::InsertLastOutputIntoAgent => {
                focused.is_some_and(|t| {
                    t.agent.is_none()
                        && t.session()
                            .is_some_and(forgeterm_terminal::Session::has_prompt_marks)
                }) && self.agent_target().is_some()
            }
            CommandId::Copy => focused
                .and_then(TerminalPane::session)
                .is_some_and(|s| s.selection_text().is_some()),
            CommandId::Paste
            | CommandId::FindInTerminal
            | CommandId::ClearScrollback
            | CommandId::ScrollToTop
            | CommandId::ScrollToBottom
            | CommandId::ScrollPageUp
            | CommandId::ScrollPageDown => focused.is_some_and(TerminalPane::is_running),
            CommandId::RestartShell
            | CommandId::ClosePane
            | CommandId::CloseTab
            | CommandId::SplitRight
            | CommandId::SplitDown => focused.is_some(),
            CommandId::NewTabWithShell(i) => i < self.shells.len(),
            CommandId::NewSshTab(i) => self.ssh_program.is_some() && i < self.ssh_hosts.len(),
            CommandId::SelectTab(i) => i < self.workspace.tabs.len() && i != self.workspace.active,
            CommandId::OpenPromptsFolder => self.prompts_dir().is_some(),
            CommandId::GitShowHistory => self.git.root().is_some(),
            CommandId::ToggleGitDashboard | CommandId::ToggleCompare => true,
            CommandId::CloseTabAt(i) => i < self.workspace.tabs.len(),
            CommandId::InsertPrompt => {
                self.prompt_studio
                    .as_ref()
                    .and_then(PromptStudio::rendered)
                    .is_some_and(|r| r.missing.is_empty() && !r.text.is_empty())
                    && focused.is_some_and(TerminalPane::is_running)
            }
            CommandId::CopyPrompt => self
                .prompt_studio
                .as_ref()
                .and_then(PromptStudio::rendered)
                .is_some_and(|r| !r.text.is_empty()),
            CommandId::ReloadPrompts => self.prompt_studio.is_some(),
            CommandId::GitCommit => self.git.can_commit(),
            CommandId::GitStageAll => {
                !self.git.action_running && !self.git.stage_all_paths().is_empty()
            }
            CommandId::GitUnstageAll => {
                !self.git.action_running && !self.git.unstage_all_paths().is_empty()
            }
            CommandId::GitTrustRepository => {
                matches!(self.git.repo, Some(Repo::Untrusted { .. }))
            }
            CommandId::GitToggleRemoteBranches => self.git.root().is_some(),
            CommandId::CompareSwap => self.compare.visible && self.compare.paths().is_some(),
            CommandId::CompareOpenTests => {
                self.compare.paths().is_some() && !self.compare.test_command.trim().is_empty()
            }
            CommandId::CompareInsertPrompt => {
                self.compare.paths().is_some()
                    && !self.compare.prompt.text().trim().is_empty()
                    && self.compare_agents().iter().any(|l| !l.is_empty())
            }
            CommandId::NewAgentWorktree => {
                matches!(self.git.repo, Some(Repo::Found { .. }))
                    && AgentKind::ALL
                        .iter()
                        .any(|k| self.found_agent(*k).is_some())
            }
            CommandId::RemoveWorktree(id) => self.removable_worktree(id).is_some(),
            CommandId::WorktreeTerminal(i) => self.worktree_path(i).is_some(),
            CommandId::ToggleViewer | CommandId::ToggleMonitor | CommandId::KeyboardShortcuts => {
                true
            }
            CommandId::ViewSelectedFile => self.explorer.selected().is_some_and(|e| !e.is_dir),
            CommandId::CommandPalette
            | CommandId::OpenPromptStudio
            | CommandId::TogglePromptStudioDock
            | CommandId::ToggleGitPanel
            | CommandId::GitRefresh
            | CommandId::NewTab
            | CommandId::ZoomIn
            | CommandId::ZoomOut
            | CommandId::ZoomReset
            | CommandId::SetTheme(_)
            | CommandId::About
            | CommandId::Website
            | CommandId::Quit
            | CommandId::ToggleExplorer
            | CommandId::ToggleAiPanel
            | CommandId::ToggleMissionControl
            | CommandId::AiRefresh => true,
            CommandId::GoToTerminal(id)
            | CommandId::TerminalGit(id)
            | CommandId::StopTerminal(id) => self.terminals.contains_key(&id),
            CommandId::TerminalFolder(id) => self
                .terminals
                .get(&id)
                .is_some_and(|t| t.directory().is_some()),
            CommandId::StartAgent(kind, _) => self.found_agent(kind).is_some(),
            CommandId::OpenSettings | CommandId::ReloadSettings => self.settings_path.is_some(),
            CommandId::ExplorerToggleHidden
            | CommandId::ExplorerRefresh
            | CommandId::ExplorerToggleFollow => self.panel_open(SidePanel::Explorer),
            CommandId::ExplorerParent => {
                self.panel_open(SidePanel::Explorer) && self.explorer.parent().is_some()
            }
            CommandId::ExplorerGoToFolder => true,
            CommandId::ExplorerOpenSelected => {
                self.panel_open(SidePanel::Explorer)
                    && self.explorer.selected().is_some_and(|e| e.is_dir)
            }
            CommandId::CopySelectedPath | CommandId::NewTabInSelectedFolder => {
                self.panel_open(SidePanel::Explorer) && self.explorer.selected().is_some()
            }
            CommandId::InsertSelectedPath => {
                self.panel_open(SidePanel::Explorer)
                    && self.explorer.selected().is_some()
                    && focused.is_some_and(TerminalPane::is_running)
            }
        }
    }

    /// Display title of a command; parameterised commands use live data.
    pub(super) fn command_title(&self, id: CommandId) -> String {
        match id {
            CommandId::NewTabWithShell(i) => self.shells.get(i).map_or_else(
                || id.static_title().to_owned(),
                |s| format!("New Tab with {}", s.name),
            ),
            CommandId::SetTheme(t) if t == theme::current() => {
                format!("Theme: {} (current)", t.name())
            }
            CommandId::SetTheme(t) => format!("Theme: {}", t.name()),
            CommandId::NewSshTab(i) => self.ssh_hosts.get(i).map_or_else(
                || id.static_title().to_owned(),
                |h| format!("New SSH Tab: {} ({})", h.alias, h.target()),
            ),
            CommandId::StartAgent(kind, false) => format!("Start {kind} in Project"),
            CommandId::StartAgent(kind, true) => format!("Start {kind} in Split"),
            CommandId::GoToTerminal(t) => match self.terminals.get(&t) {
                Some(term) => {
                    let tab = self.workspace.locate(t).map_or(0, |(i, _)| i + 1);
                    let name = term
                        .agent
                        .map_or_else(|| term.display_title(), |k| k.name().to_owned());
                    let project = term
                        .directory()
                        .and_then(Path::file_name)
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    format!("Go to {name} — {project} (tab {tab})")
                }
                None => id.static_title().to_owned(),
            },
            _ => id.static_title().to_owned(),
        }
    }

    /// Entries of menu `index`, resolved against the current state.
    pub(super) fn menu_rows(&self, index: usize) -> Vec<menu::Row> {
        let Some(m) = self.menus.get(index) else {
            return Vec::new();
        };
        m.items
            .iter()
            .map(|item| match *item {
                menu::Item::Separator => menu::Row::Separator,
                menu::Item::Command(id) => menu::Row::Command {
                    id,
                    label: self.command_title(id),
                    hint: self.keymap.binding_for(id).map(|b| b.to_string()),
                    enabled: self.is_applicable(id),
                },
            })
            .collect()
    }

    pub(super) fn palette_entries(&self) -> Vec<Entry> {
        let entry = |id: CommandId, title: String| Entry {
            id,
            label: format!("{}: {}", id.category(), title),
            hint: self.keymap.binding_for(id).map(|b| b.to_string()),
        };
        let mut entries: Vec<Entry> = STATIC_COMMANDS
            .iter()
            .map(|&id| entry(id, id.static_title().to_owned()))
            .collect();
        for i in 0..self.shells.len() {
            let id = CommandId::NewTabWithShell(i);
            entries.push(entry(id, self.command_title(id)));
        }
        for kind in AgentKind::ALL {
            for split in [false, true] {
                let id = CommandId::StartAgent(kind, split);
                entries.push(entry(id, self.command_title(id)));
            }
        }
        for i in 0..self.ssh_hosts.len() {
            let id = CommandId::NewSshTab(i);
            entries.push(entry(id, self.command_title(id)));
        }
        for t in ThemeChoice::ALL {
            let id = CommandId::SetTheme(t);
            entries.push(entry(id, self.command_title(id)));
        }
        for id in self.mission_order() {
            let id = CommandId::GoToTerminal(id);
            entries.push(entry(id, self.command_title(id)));
        }
        for (i, tab) in self.workspace.tabs.iter().enumerate() {
            let title = tab
                .focused()
                .and_then(|id| self.terminals.get(&id))
                .map_or_else(|| "—".to_owned(), TerminalPane::display_title);
            entries.push(entry(
                CommandId::SelectTab(i),
                format!("Go to Tab {}: {}", i + 1, title),
            ));
        }
        entries.retain(|e| self.is_applicable(e.id));
        entries
    }
}
