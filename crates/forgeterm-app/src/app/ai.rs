//! Agents and the Prompt Studio: starting, inserting (never with Enter).

use super::*;

impl App {
    /// The docked studio takes or returns the keyboard; the focused terminal redraws
    /// its cursor (hollow while the keyboard is elsewhere).
    pub(super) fn set_prompt_focused(&mut self, focused: bool) {
        if self.prompt_focused != focused {
            self.prompt_focused = focused;
            if let Some(t) = self
                .workspace
                .focused()
                .and_then(|id| self.terminals.get(&id))
            {
                t.cache.clear();
            }
        }
    }

    /// Rescans the project context when the AI panel is visible and the focused
    /// shell's folder changed.
    pub(super) fn sync_ai_context(&mut self) -> Option<Task<Message>> {
        if !self.panel_open(SidePanel::Ai) {
            return None;
        }
        let dir = self.focused_cwd().unwrap_or_else(|| self.start_dir.clone());
        if self.ai_context_for.as_ref() == Some(&dir) {
            return None;
        }
        self.ai_context_for = Some(dir.clone());
        Some(scan_context(dir))
    }

    pub(super) fn found_agent(&self, kind: AgentKind) -> Option<&Agent> {
        self.agents.iter().find_map(|(k, s)| match s {
            AgentState::Found(a) if *k == kind => Some(a),
            _ => None,
        })
    }

    /// Starts an agent in the focused shell's project root, as a new tab or split.
    pub(super) fn start_agent(&mut self, kind: AgentKind, split: bool) -> Task<Message> {
        let here = self.focused_cwd().unwrap_or_else(|| self.start_dir.clone());
        let root = forgeterm_agents::project_root(&here).0;
        self.start_agent_in(kind, root, split)
    }

    /// Starts an agent in `root` (a project root or worktree), as a new tab or split.
    pub(super) fn start_agent_in(
        &mut self,
        kind: AgentKind,
        root: PathBuf,
        split: bool,
    ) -> Task<Message> {
        let Some(agent) = self.found_agent(kind) else {
            return Task::none();
        };
        let launch = match forgeterm_agents::launch_command(&agent.path) {
            Ok(l) => l,
            Err(e) => {
                self.notice = Some(e);
                return Task::none();
            }
        };
        let shell = ShellSpec::new(kind.name(), launch.program, launch.args);
        if split {
            self.restore_zoom();
        }
        let id = self.spawn_terminal_for(shell, Some(root), Some(kind));
        if split {
            let ok = self
                .workspace
                .active_tab_mut()
                .and_then(|tab| tab.split(Axis::Vertical, id))
                .is_some();
            if !ok {
                self.terminals.remove(&id);
                return Task::none();
            }
            self.refresh_visible();
        } else {
            self.workspace.push_tab(id);
        }
        Task::none()
    }

    /// `<settings dir>/prompts`, or `None` without a config directory.
    pub(super) fn prompts_dir(&self) -> Option<PathBuf> {
        self.settings_path
            .as_deref()
            .and_then(forgeterm_prompts::user_dir)
    }

    pub(super) fn open_prompt_studio(&mut self) -> Task<Message> {
        self.menu = None;
        self.command_palette = None;
        let auto = self.auto_prompt_values();
        self.prompt_studio = Some(PromptStudio::new(auto));
        load_prompts(self.prompts_dir())
    }

    /// Values ForgeTerm really knows; anything unknown is simply left out.
    pub(super) fn auto_prompt_values(&self) -> HashMap<String, String> {
        let dir = self.focused_cwd().unwrap_or_else(|| self.start_dir.clone());
        let root = forgeterm_agents::project_root(&dir).0;
        let mut values = HashMap::new();
        if let Some(name) = root.file_name() {
            values.insert("project".to_owned(), name.to_string_lossy().into_owned());
        }
        values.insert("project_path".to_owned(), root.display().to_string());
        values.insert("cwd".to_owned(), dir.display().to_string());
        if let Some(selection) = self
            .focused()
            .and_then(TerminalPane::session)
            .and_then(|s| s.selection_text())
        {
            values.insert("selection".to_owned(), selection);
        }
        if let Some(file) = self.explorer.selected().filter(|e| !e.is_dir) {
            let shown = file.path.strip_prefix(&root).map_or_else(
                |_| file.path.display().to_string(),
                |rel| rel.to_string_lossy().replace('\\', "/"),
            );
            values.insert("file".to_owned(), shown);
        }
        values
    }

    /// The terminal Prompt Studio inserts into: the focused one, while it runs. The
    /// floating studio is modal, so that is the terminal focused when it opened.
    pub(super) fn prompt_target(&self) -> Option<&TerminalPane> {
        self.focused().filter(|t| t.is_running())
    }

    /// The agent that receives inserted command output: a running agent terminal in
    /// the active tab, else the first one in another tab.
    pub(super) fn agent_target(&self) -> Option<TermId> {
        let is_agent = |id: &TermId| {
            self.terminals
                .get(id)
                .is_some_and(|t| t.agent.is_some() && t.is_running())
        };
        self.workspace
            .active_tab()
            .and_then(|tab| tab.terminals().find(is_agent))
            .or_else(|| {
                self.workspace
                    .tabs
                    .iter()
                    .flat_map(|tab| tab.terminals())
                    .find(is_agent)
            })
    }

    /// Puts the focused shell's last command output (Markdown, see
    /// `CommandOutput::to_markdown`) at the agent's prompt, like a paste and without
    /// Enter, then gives the agent the keyboard so the user can add a question.
    pub(super) fn insert_last_output_into_agent(&mut self) -> Task<Message> {
        let Some(session) = self.focused().and_then(TerminalPane::session) else {
            return Task::none();
        };
        let output = match session.last_command_output() {
            Ok(out) => out,
            Err(e) => {
                self.notice = Some(format!("Nothing inserted: {e}."));
                return Task::none();
            }
        };
        let Some(target) = self.agent_target() else {
            self.notice = Some("Nothing inserted: no agent is running.".into());
            return Task::none();
        };
        if let Some((index, _)) = self.workspace.locate(target)
            && self.workspace.select(index)
        {
            self.refresh_visible();
        }
        self.focus_terminal(target);
        self.insert_text_into(target, output.to_markdown());
        unfocus_widgets()
    }

    /// Inserts the rendered prompt into the focused terminal, without Enter, and
    /// gives that terminal the keyboard. The docked studio stays open.
    pub(super) fn insert_prompt(&mut self) -> Task<Message> {
        let Some(r) = self
            .prompt_studio
            .as_ref()
            .and_then(PromptStudio::rendered)
            .filter(|r| r.missing.is_empty() && !r.text.is_empty())
        else {
            return Task::none();
        };
        // No running terminal (it ended meanwhile): the text stays, Copy still works.
        let Some(id) = self.prompt_target().map(|t| t.id) else {
            return Task::none();
        };
        if !self.prompt_docked {
            self.prompt_studio = None;
        }
        self.focus_terminal(id);
        self.insert_text_into(id, r.text);
        unfocus_widgets()
    }

    /// Inserts paths quoted for the focused terminal's shell, followed by a space.
    /// Inserts a project file into the focused terminal: as an `@path` file
    /// reference (relative to the project root) when it runs Claude Code, otherwise
    /// as a path quoted for the shell. Never followed by Enter.
    pub(super) fn insert_context_file(&mut self, path: &Path) {
        let agent = self.focused().and_then(|t| t.agent);
        match agent {
            Some(AgentKind::ClaudeCode) => {
                let root = self.ai_context.as_ref().map(|c| c.root.clone());
                let rel = root
                    .as_deref()
                    .and_then(|r| path.strip_prefix(r).ok())
                    .map_or_else(|| path.to_path_buf(), Path::to_path_buf);
                let text = rel.to_string_lossy().replace('\\', "/");
                // A reference with spaces would end at the first space.
                if text.contains(char::is_whitespace) {
                    self.insert_text(format!("{} ", path.display()));
                } else {
                    self.insert_text(format!("@{text} "));
                }
            }
            Some(AgentKind::Codex) => self.insert_text(format!("{} ", path.display())),
            None => self.insert_paths(&[path.to_path_buf()]),
        }
    }

    pub(super) fn insert_paths(&mut self, paths: &[PathBuf]) {
        // The path lands in the terminal: so does the keyboard.
        self.explorer_focused = false;
        let Some(kind) = self.focused().map(|t| t.shell.kind) else {
            return;
        };
        let mut text = String::new();
        for p in paths {
            text.push_str(&quote_path(kind, &p.to_string_lossy()));
            text.push(' ');
        }
        self.insert_text(text);
    }
}
