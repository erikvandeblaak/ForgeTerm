//! The message handler: every `Message` lands here.

use super::*;

impl App {
    pub(super) fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Session {
                id,
                generation,
                event,
            } => {
                // Mission Control covers the panes: nothing there is visible or
                // "seen"; agent previews refresh once per second (MissionTick).
                let visible = !self.main_covered() && self.workspace.is_visible(id);
                let seen = visible && self.window_focused;
                let Some(terminal) = self.terminals.get_mut(&id) else {
                    return Task::none();
                };
                match terminal.on_session_event(generation, event, visible, seen, &self.palette) {
                    Outcome::None => {}
                    // The tab shows it; outside the window the taskbar button flashes.
                    Outcome::Attention if !self.window_focused => {
                        return iced::window::latest().and_then(|w| {
                            iced::window::request_user_attention(
                                w,
                                Some(iced::window::UserAttention::Informational),
                            )
                        });
                    }
                    Outcome::Attention => {}
                    Outcome::Clipboard(text) => return self.program_clipboard(id, text),
                    Outcome::Close => return self.close_terminal(id),
                    // Picked up by `sync_explorer` after this message.
                    Outcome::CwdChanged => {}
                }
            }
            Message::Term(id, msg) => return self.on_term_msg(id, msg),
            Message::KeyPressed {
                key,
                text,
                modifiers,
                captured,
            } => return self.on_key(key, text, modifiers, captured),
            Message::ShellPicked(choice) => return self.run_command(choice.command),
            // Through the command, so a page covering the terminals (Mission Control,
            // Git Control Center) closes too, also when the active tab is clicked.
            Message::SelectTab(index) => return self.run_command(CommandId::SelectTab(index)),
            Message::WindowCloseRequested => return self.request_close(CloseTarget::Quit),
            Message::CloseChecked(target, programs) => {
                if programs.is_empty() {
                    return self.perform_close(target);
                }
                self.pending_close = Some((target, programs));
            }
            Message::ConfirmClose => {
                if let Some((target, _)) = self.pending_close.take() {
                    return self.perform_close(target);
                }
            }
            Message::CancelClose => self.pending_close = None,
            Message::TermFindQuery(query) => {
                // Search as you type, after a short pause (a long scrollback takes
                // milliseconds per search).
                let Some(f) = &mut self.term_find else {
                    return Task::none();
                };
                f.query = query;
                f.seq += 1;
                f.searched = false;
                f.outcome = None;
                let seq = f.seq;
                return background(
                    move || {
                        std::thread::sleep(TERM_FIND_DEBOUNCE);
                        seq
                    },
                    Message::TermFindDue,
                );
            }
            Message::TermFindDue(seq) => {
                if self
                    .term_find
                    .as_ref()
                    .is_some_and(|f| f.seq == seq && !f.searched)
                {
                    self.term_find_first();
                }
            }
            Message::TermFindStep(older) => {
                let Some(f) = &self.term_find else {
                    return Task::none();
                };
                if !f.searched {
                    // Enter before the pause ended: search now.
                    self.term_find_first();
                } else {
                    let query = f.query.clone();
                    let outcome = self.term_search(&query, older);
                    if let Some(f) = &mut self.term_find {
                        f.outcome = outcome;
                    }
                }
            }
            Message::TermFindClose => self.term_find = None,
            Message::DismissNotice => self.notice = None,
            Message::DockerListed(result, explicit) => return self.docker_listed(result, explicit),
            Message::DockerDone(result, what) => return self.docker_done(result, what),
            Message::MonitorSampled(sample) => {
                self.record_mission_cpu(&sample);
                self.monitor.record(*sample);
            }
            Message::MissionTick => {
                self.mission.ticks = self.mission.ticks.wrapping_add(1);
                for t in self.terminals.values_mut().filter(|t| t.agent.is_some()) {
                    t.refresh_if_dirty(&self.palette);
                }
                if self.mission.ticks.is_multiple_of(5) {
                    return self.read_mission_heads();
                }
            }
            Message::MissionHeads(heads) => {
                self.mission.heads_busy = false;
                self.mission.heads.extend(heads);
            }
            Message::DebugScreenshot => {
                return iced::window::latest()
                    .and_then(iced::window::screenshot)
                    .map(Message::DebugScreenshotTaken);
            }
            Message::DebugScreenshotTaken(shot) => {
                if let Some(path) = std::env::var_os("FORGETERM_SCREENSHOT") {
                    save_screenshot(&shot, Path::new(&path));
                }
            }
            Message::FocusPane(pane) => {
                if let Some(tab) = self.workspace.active_tab_mut()
                    && tab.focus != pane
                {
                    tab.focus = pane;
                    self.refresh_visible();
                }
            }
            Message::PaneResized(pane_grid::ResizeEvent { split, ratio }) => {
                if let Some(tab) = self.workspace.active_tab_mut() {
                    tab.panes.resize(split, ratio);
                }
            }
            Message::Paste => return clipboard::read().map(Message::ClipboardText),
            Message::ClipboardText(Some(text)) => self.insert_text(text),
            Message::ClipboardText(None) => {}
            Message::ConfirmPaste => {
                if let Some((id, text)) = self.pending_paste.take()
                    && let Some(s) = self.terminals.get(&id).and_then(TerminalPane::session)
                {
                    s.paste(&text);
                }
            }
            Message::CancelPaste => self.pending_paste = None,
            Message::Command(id) => return self.run_command(id),
            Message::PaletteQuery(query) => {
                if let Some(p) = &mut self.command_palette {
                    p.set_query(query);
                }
            }
            Message::PaletteHover(index) => {
                if let Some(p) = &mut self.command_palette {
                    let delta = index as i32 - p.selected_index() as i32;
                    p.move_selection(delta);
                }
            }
            Message::PaletteRun(index) => {
                let id = self.command_palette.as_ref().and_then(|p| match index {
                    Some(i) => p.command_at(i),
                    None => p.selected(),
                });
                self.command_palette = None;
                if let Some(id) = id {
                    return self.run_command(id);
                }
            }
            Message::PaletteClose => self.command_palette = None,
            Message::MenuOpen(index) => {
                self.menu = match self.menu {
                    Some(state) if state.open == index => None,
                    _ => Some(MenuState::new(index)),
                };
            }
            Message::MenuHoverTitle(index) => {
                if let Some(state) = &mut self.menu
                    && state.open != index
                {
                    *state = MenuState::new(index);
                }
            }
            Message::MenuHoverItem(index) => {
                if let Some(state) = &mut self.menu {
                    state.highlighted = Some(index);
                }
            }
            Message::MenuRun(index) => {
                let Some(state) = self.menu else {
                    return Task::none();
                };
                let rows = self.menu_rows(state.open);
                let run = MenuState {
                    highlighted: Some(index),
                    ..state
                };
                self.menu = None;
                if let Some(id) = run.highlighted_command(&rows) {
                    return self.run_command(id);
                }
            }
            Message::MenuClose => self.menu = None,
            Message::AboutClose => self.about_open = false,
            Message::FileRefResolved {
                wanted,
                line,
                found,
            } => match found {
                Some(path) => {
                    self.viewer.pending_line =
                        line.map(|l| (path.clone(), (l as usize).saturating_sub(1)));
                    return self.viewer_msg(ViewerMsg::Open(path));
                }
                None => {
                    self.notice = Some(format!(
                        "Not opened: {wanted} is not a file in this terminal's folder or its project."
                    ));
                }
            },
            Message::WindowFocus(focused) => {
                let back = focused && !self.window_focused;
                self.window_focused = focused;
                // The idle timers pause in the background: catch up once.
                if back {
                    return Task::batch([
                        Task::done(Message::SettingsTick),
                        self.refresh_git(false),
                    ]);
                }
            }
            Message::ShortcutsClose => self.shortcuts_open = false,
            Message::ExplorerListed(path, result) => self.explorer.set_listing(path, result),
            Message::ExplorerSelect(path) => {
                self.explorer.select(Some(path));
                self.focus_explorer();
            }
            Message::ExplorerToggle(path) => {
                self.explorer.select(Some(path.clone()));
                self.focus_explorer();
                return self
                    .explorer
                    .toggle(&path)
                    .map_or_else(Task::none, load_dir);
            }
            Message::ExplorerActivate(path, true) => {
                return self
                    .explorer
                    .toggle(&path)
                    .map_or_else(Task::none, load_dir);
            }
            Message::ExplorerActivate(path, false) => self.insert_paths(&[path]),
            Message::ExplorerTick => {
                if self.panel_open(SidePanel::Explorer) && !self.explorer_checking {
                    let shown = self.explorer.shown_dirs();
                    if !shown.is_empty() {
                        self.explorer_checking = true;
                        return background(
                            move || forgeterm_explorer::changed_dirs(&shown),
                            Message::ExplorerChanged,
                        );
                    }
                }
            }
            Message::ExplorerChanged(dirs) => {
                self.explorer_checking = false;
                return Task::batch(dirs.into_iter().map(load_dir));
            }
            Message::ExplorerScrolled(viewport) => {
                self.explorer_scroll = (viewport.absolute_offset().y, viewport.bounds().height);
            }
            Message::ExplorerGoToInput(text) => {
                if let Some(goto) = &mut self.explorer_goto {
                    *goto = (text, None);
                }
            }
            Message::ExplorerGoToSubmit => {
                let Some((text, _)) = &self.explorer_goto else {
                    return Task::none();
                };
                let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                    .map(PathBuf::from);
                let wanted = forgeterm_explorer::folder_from_input(
                    text,
                    self.explorer.root(),
                    home.as_deref(),
                );
                return background(
                    move || {
                        let dir = wanted?;
                        // Same rule as folders reported by shells: never a network path.
                        if !dir.to_str().is_some_and(forgeterm_terminal::is_local_path)
                            || !forgeterm_fs::resolves_locally(&dir)
                        {
                            return Err(format!("{} is not a local folder.", dir.display()));
                        }
                        if dir.is_dir() {
                            Ok(dir)
                        } else {
                            Err(format!("{} is not a folder.", dir.display()))
                        }
                    },
                    Message::ExplorerGoToResult,
                );
            }
            Message::ExplorerGoToResult(result) => match result {
                Ok(dir) => {
                    self.explorer_goto = None;
                    self.side = Some(SidePanel::Explorer);
                    self.focus_explorer();
                    return Task::batch([load_dir(self.explorer.set_root(dir)), unfocus_widgets()]);
                }
                Err(e) => {
                    if let Some(goto) = &mut self.explorer_goto {
                        goto.1 = Some(e);
                    }
                }
            },
            Message::FileHovered => self.drop_hover = true,
            Message::FilesHoveredLeft => self.drop_hover = false,
            Message::SettingsTick => {
                if self.settings_changed_on_disk() {
                    self.load_settings(false);
                }
                self.save_session();
                self.load_ssh_hosts_if_changed();
                let sample = self.sample_monitor();
                // Files open in the viewer that changed on disk are reloaded.
                let changed: Vec<PathBuf> = self
                    .viewer
                    .tabs
                    .iter_mut()
                    .filter(|t| !t.reloading)
                    .filter(|t| {
                        let now = forgeterm_viewer::modified(&t.doc.path);
                        now.is_some() && now != t.doc.modified
                    })
                    .map(|t| {
                        t.reloading = true;
                        t.doc.path.clone()
                    })
                    .collect();
                return Task::batch(changed.into_iter().map(load_document).chain(sample));
            }
            Message::DismissSettingsProblems => self.settings_problems.clear(),
            Message::WorkerPanicked => {
                // Which task it was is unknown here: free every "running" flag, so
                // nothing stays stuck (at worst one job runs twice), and say so.
                tracing::error!("a background task panicked");
                self.git.refreshing = None;
                self.git.action_running = false;
                self.monitor.busy = false;
                self.explorer_checking = false;
                self.git_dash.busy = false;
                self.mission.heads_busy = false;
                self.notice =
                    Some("A background task failed unexpectedly; it will be retried.".to_owned());
            }
            Message::AgentsDetected(found) => {
                self.agents = found
                    .into_iter()
                    .map(|(k, a)| (k, a.map_or(AgentState::Missing, AgentState::Found)))
                    .collect();
            }
            Message::ContextScanned(dir, ctx) => {
                if self.ai_context_for.as_ref() == Some(&dir) {
                    self.ai_context = Some(ctx);
                }
            }
            Message::AiInsert(path) => self.insert_context_file(&path),
            Message::PromptsLoaded(library) => {
                if let Some(studio) = &mut self.prompt_studio {
                    studio.set_library(library);
                }
            }
            Message::PromptSelect(index) => {
                if let Some(studio) = &mut self.prompt_studio {
                    studio.select(index);
                }
            }
            Message::PromptEdit(name, action) => {
                if let Some(studio) = &mut self.prompt_studio {
                    studio.edit(name, action);
                    // Clicking or typing in a field: the keyboard belongs to the studio.
                    self.set_prompt_focused(self.prompt_docked);
                }
            }
            Message::PromptClose => {
                self.prompt_studio = None;
                self.set_prompt_focused(false);
            }
            Message::GitTick => return self.refresh_git(false),
            Message::GitRefreshed(snapshot) => {
                if self.git.refreshing.as_ref() == Some(&snapshot.dir) {
                    self.git.refreshing = None;
                }
                // A result for a folder the terminal has left is dropped; the
                // follow-up sync starts a refresh for the new one.
                if snapshot.dir == self.git_dir() {
                    self.git.apply(*snapshot);
                    return self.load_git_dash(false);
                }
            }
            Message::WorktreeFormAgent(kind) => {
                if let Some(f) = &mut self.worktree_form {
                    if worktree_form::is_suggestion(&f.branch) {
                        f.branch = worktree_form::suggest_branch(kind, &f.existing);
                    }
                    f.kind = kind;
                }
            }
            Message::WorktreeFormBranches(names) => {
                if let Some(f) = &mut self.worktree_form
                    && f.set_existing(names)
                {
                    return iced::widget::operation::select_all(worktree_form::BRANCH_INPUT_ID);
                }
            }
            Message::WorktreeFormBranch(branch) => {
                if let Some(f) = &mut self.worktree_form {
                    f.branch = branch;
                    f.error = None;
                }
            }
            Message::WorktreeFormSubmit => {
                let Some(f) = self.worktree_form.as_mut().filter(|f| !f.busy) else {
                    return Task::none();
                };
                let Some(path) = forgeterm_git::default_worktree_path(&f.root, &f.branch) else {
                    f.error = Some("Type a branch name.".to_owned());
                    return Task::none();
                };
                f.busy = true;
                f.error = None;
                let (root, branch) = (f.root.clone(), f.branch.trim().to_owned());
                return background(
                    move || forgeterm_git::add_worktree(&root, &path, &branch).map(|()| path),
                    Message::WorktreeCreated,
                );
            }
            Message::WorktreeFormCancel => {
                if self.worktree_form.as_ref().is_some_and(|f| !f.busy) {
                    self.worktree_form = None;
                }
            }
            Message::WorktreeCreated(Ok(path)) => {
                let Some(form) = self.worktree_form.take() else {
                    return Task::none();
                };
                self.mission.visible = false;
                self.git_dash.visible = false;
                self.compare.visible = false;
                self.refresh_visible();
                if self.found_agent(form.kind).is_none() {
                    self.notice = Some(format!(
                        "Worktree created at {}, but {} was not found: no agent started.",
                        path.display(),
                        form.kind.name()
                    ));
                }
                let start = self.start_agent_in(form.kind, path, false);
                return Task::batch([start, self.refresh_git(true)]);
            }
            Message::WorktreeCreated(Err(e)) => {
                if let Some(f) = &mut self.worktree_form {
                    f.busy = false;
                    f.error = Some(e);
                }
            }
            Message::WorktreeRemoveConfirm => {
                if let Some((root, path, _)) = self.pending_worktree_remove.take() {
                    return background(
                        move || forgeterm_git::remove_worktree(&root, &path),
                        Message::WorktreeRemoved,
                    );
                }
            }
            Message::WorktreeRemoveCancel => self.pending_worktree_remove = None,
            Message::CompareLoaded(data) => {
                if data.seq != self.compare.seq {
                    return Task::none();
                }
                self.compare.busy = false;
                if self
                    .compare
                    .data
                    .as_ref()
                    .is_some_and(|d| d.root != data.root)
                {
                    self.compare.test_command_edited = false;
                }
                if !self.compare.test_command_edited {
                    self.compare.test_command = data.test_command.unwrap_or_default().to_owned();
                }
                if let Some([a, b]) = &data.sides {
                    self.compare.a = Some(a.worktree.path.clone());
                    self.compare.b = Some(b.worktree.path.clone());
                }
                let still = data.sides.as_ref().is_some_and(|[a, b]| {
                    let has = |s: &compare_view::Side, p: &str| {
                        s.files
                            .as_ref()
                            .is_ok_and(|f| f.iter().any(|f| f.path == p))
                    };
                    self.compare
                        .selected
                        .as_deref()
                        .is_some_and(|p| has(a, p) || has(b, p))
                });
                self.compare.data = Some(*data);
                if let Some(path) = self.compare.selected.clone() {
                    if still {
                        // Its contents may have changed: read it again (the old rows
                        // stay until then).
                        return self.load_compare_diff(path, false);
                    }
                    self.compare.selected = None;
                    self.compare.diff = None;
                }
            }
            Message::ComparePickA(c) => {
                if self.compare.b.as_ref() == Some(&c.path) {
                    self.compare.b = self.compare.a.take();
                }
                self.compare.a = Some(c.path);
                return self.compare_sides_changed();
            }
            Message::ComparePickB(c) => {
                if self.compare.a.as_ref() == Some(&c.path) {
                    self.compare.a = self.compare.b.take();
                }
                self.compare.b = Some(c.path);
                return self.compare_sides_changed();
            }
            Message::CompareSelect(path) => return self.load_compare_diff(path, true),
            Message::CompareDiffLoaded(pair, path, result) => {
                // Only for the pair shown now (sides may have been picked or swapped
                // meanwhile).
                if self.compare.paths().as_ref() == Some(&pair)
                    && let Some(d) = &mut self.compare.diff
                    && d.path == path
                {
                    d.result = Some(result);
                }
            }
            Message::CompareTestCommand(command) => {
                self.compare.test_command = command;
                self.compare.test_command_edited = true;
            }
            Message::ComparePromptEdit(action) => self.compare.prompt.perform(action),
            Message::CompareTick => {
                self.process_pending_inserts();
                if self.compare.visible && !self.compare.busy {
                    let root = self.git.root().cloned();
                    let stale = self.compare.data.as_ref().is_none_or(|d| {
                        Some(&d.root) != root.as_ref()
                            || d.loaded_at.elapsed() > Duration::from_secs(10)
                    });
                    if stale {
                        return self.load_compare();
                    }
                }
            }
            Message::WorktreeRemoved(result) => {
                match result {
                    Err(e) => self.notice = Some(format!("{REMOVE_FAILED}{e}")),
                    // An earlier refusal no longer applies.
                    Ok(()) => {
                        if self
                            .notice
                            .as_deref()
                            .is_some_and(|n| n.starts_with(REMOVE_FAILED))
                        {
                            self.notice = None;
                        }
                    }
                }
                return self.load_git_dash(true);
            }
            Message::GitDashLoaded(data) => {
                self.git_dash.busy = false;
                self.git_dash.data = Some(*data);
                if std::mem::take(&mut self.git_dash.reload_pending) {
                    return self.load_git_dash(true);
                }
            }
            Message::GitStage(paths) => return self.git_action(GitAction::Stage(paths)),
            Message::GitUnstage(paths) => return self.git_action(GitAction::Unstage(paths)),
            Message::GitCommitEdit(action) => self.git.commit_message.perform(action),
            Message::GitActionDone(result) => {
                self.git.action_running = false;
                if result.is_ok() && self.git.last_action_was_commit {
                    self.git.commit_message = iced::widget::text_editor::Content::new();
                }
                if result.is_ok() && self.git.last_action_was_create {
                    self.git.new_branch.clear();
                }
                self.git.last_action = Some(result);
                return self.refresh_git(true);
            }
            Message::GitShowDiff(path, kind) => {
                let Some(root) = self.git.root().cloned() else {
                    return Task::none();
                };
                self.git.diff = Some(DiffView {
                    path: path.clone(),
                    kind,
                    result: None,
                });
                return background(
                    {
                        let path = path.clone();
                        move || forgeterm_git::diff(&root, &path, kind)
                    },
                    move |result| Message::GitDiffLoaded(path.clone(), kind, result),
                );
            }
            Message::GitDiffLoaded(path, kind, result) => {
                if let Some(d) = &mut self.git.diff
                    && d.path == path
                    && d.kind == kind
                {
                    d.result = Some(result);
                }
            }
            Message::GitDiffClose => self.git.diff = None,
            Message::GitHistoryLoaded(data) => {
                if self.git.root() == Some(&data.root)
                    && let Some(h) = &mut self.git.history
                {
                    h.entries = Some(data.entries);
                    h.activity = Some(data.activity);
                }
            }
            Message::GitSelectCommit(hash) => {
                let (Some(root), Some(h)) = (self.git.root().cloned(), &mut self.git.history)
                else {
                    return Task::none();
                };
                h.selected = Some(hash.clone());
                h.detail = None;
                return background(
                    {
                        let hash = hash.clone();
                        move || forgeterm_git::show_commit(&root, &hash)
                    },
                    move |result| Message::GitCommitLoaded(hash.clone(), result),
                );
            }
            Message::GitCommitLoaded(hash, result) => {
                if let Some(h) = &mut self.git.history
                    && h.selected.as_deref() == Some(hash.as_str())
                {
                    h.detail = Some(result);
                }
            }
            Message::GitHistoryClose => self.git.history = None,
            Message::GitSwitchAsk(name) => self.git.pending_switch = Some(name),
            Message::GitSwitchCancel => self.git.pending_switch = None,
            Message::GitSwitchConfirm => {
                if let Some(name) = self.git.pending_switch.take() {
                    return self.git_action(GitAction::Switch(name));
                }
            }
            Message::GitTrack(name) => return self.git_action(GitAction::Track(name)),
            Message::GitNewBranchName(name) => self.git.new_branch = name,
            Message::GitCreateBranch => {
                let name = self.git.new_branch.trim().to_owned();
                if !name.is_empty() {
                    return self.git_action(GitAction::Create(name));
                }
            }
            Message::GitOpenInViewer(rel) => {
                if let Some(root) = self.git.root().cloned() {
                    self.git.diff = None;
                    return self.viewer_msg(ViewerMsg::Open(root.join(rel)));
                }
            }
            Message::Viewer(msg) => return self.viewer_msg(msg),
            Message::FileDropped(path) => {
                self.drop_hover = false;
                self.insert_paths(&[path]);
            }
        }
        Task::none()
    }
}
