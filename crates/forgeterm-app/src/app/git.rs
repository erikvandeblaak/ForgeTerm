//! The Git panel and the Git Control Center data.

use super::*;

impl App {
    /// Folder of the worktree with this id in the dashboard's list, when it is
    /// still there and usable.
    pub(super) fn worktree_path(&self, id: u64) -> Option<PathBuf> {
        let list = self.git_dash.data.as_ref()?.worktrees.as_ref().ok()?;
        let info = list
            .iter()
            .find(|i| git_dash::worktree_id(&i.worktree.path) == id)?;
        let usable = !info.worktree.bare
            && !matches!(
                info.changes,
                WorktreeChanges::Missing | WorktreeChanges::NotLocal
            );
        usable.then(|| info.worktree.path.clone())
    }

    /// Branch names known from the dashboard or the Git panel (for suggestions).
    pub(super) fn branch_names(&self) -> Vec<String> {
        let from = |b: &Result<Vec<forgeterm_git::BranchRef>, String>| {
            b.as_ref()
                .map(|l| l.iter().map(|b| b.name.clone()).collect::<Vec<_>>())
                .ok()
        };
        self.git_dash
            .data
            .as_ref()
            .and_then(|d| from(&d.branches))
            .or_else(|| self.git.branches.as_ref().and_then(from))
            .unwrap_or_default()
    }

    /// A linked worktree that may be removed: not the main one, not the one shown,
    /// readable, and no agent of this window working in it.
    pub(super) fn removable_worktree(&self, id: u64) -> Option<(PathBuf, PathBuf, Option<String>)> {
        let data = self.git_dash.data.as_ref()?;
        let list = data.worktrees.as_ref().ok()?;
        let (index, info) = list
            .iter()
            .enumerate()
            .find(|(_, i)| git_dash::worktree_id(&i.worktree.path) == id)?;
        let w = &info.worktree;
        let shown = git_dash::path_key(&w.path) == git_dash::path_key(&data.root);
        // The deepest worktree containing each terminal (a worktree may sit in another).
        let paths: Vec<PathBuf> = list.iter().map(|i| i.worktree.path.clone()).collect();
        let terminals = self
            .terminals
            .values()
            .filter_map(TerminalPane::directory)
            .filter(|d| git_dash::worktree_of(d, &paths) == Some(index))
            .count();
        (git_dash::removal(index == 0, shown, info, terminals) == git_dash::Removal::Allowed)
            .then(|| (data.root.clone(), w.path.clone(), w.branch.clone()))
    }

    /// Reloads the dashboard's data when it is open and out of date: a different
    /// status than at the last load (a commit, stage or edit), another repository,
    /// or older than 10 s (other worktrees, commits made elsewhere). `force`: always.
    pub(super) fn load_git_dash(&mut self, force: bool) -> Task<Message> {
        if !self.git_dash.visible {
            return Task::none();
        }
        if self.git_dash.busy {
            self.git_dash.reload_pending |= force;
            return Task::none();
        }
        let Some(Repo::Found { root, status }) = self.git.repo.as_ref() else {
            return Task::none();
        };
        let fresh =
            self.git_dash.data.as_ref().is_some_and(|d| {
                &d.root == root && d.loaded_at.elapsed() < Duration::from_secs(10)
            }) && self.git_dash.loaded_for.as_ref() == Some(status);
        if fresh && !force {
            return Task::none();
        }
        self.git_dash.busy = true;
        self.git_dash.loaded_for = Some(status.clone());
        let root = root.clone();
        let trusted: Vec<PathBuf> = self.git.trusted.iter().cloned().collect();
        background(
            move || Box::new(load_dashboard(root, &trusted)),
            Message::GitDashLoaded,
        )
    }

    pub(super) fn view_git_dash(&self) -> Element<'_, Message> {
        let mut agents = Vec::new();
        for (tab, t) in self.workspace.tabs.iter().enumerate() {
            for id in t.terminals() {
                if let Some(term) = self.terminals.get(&id)
                    && let (Some(kind), Some(dir)) = (term.agent, term.directory())
                {
                    agents.push(git_dash::AgentAt {
                        id,
                        kind,
                        tab,
                        dir: dir.to_path_buf(),
                    });
                }
            }
        }
        git_dash::view(git_dash::View {
            repo: self.git.repo.as_ref(),
            dash: &self.git_dash,
            agents,
            terminal_dirs: self
                .terminals
                .values()
                .filter_map(|t| t.directory().map(Path::to_path_buf))
                .collect(),
            panel_hint: self.keymap.hint(CommandId::ToggleGitPanel),
            compare_hint: self.keymap.hint(CommandId::ToggleCompare),
            dir: self.focused().and_then(TerminalPane::directory),
            now: std::time::Instant::now(),
        })
    }

    /// Folder git status is shown for: the focused terminal's.
    pub(super) fn git_dir(&self) -> PathBuf {
        self.focused_cwd().unwrap_or_else(|| self.start_dir.clone())
    }

    /// Refreshes when the focused terminal moved to another folder.
    pub(super) fn sync_git(&mut self) -> Option<Task<Message>> {
        let dir = self.git_dir();
        if self.git.dir.as_ref() == Some(&dir) || self.git.refreshing.as_ref() == Some(&dir) {
            return None;
        }
        Some(self.start_git_refresh(dir, None))
    }

    /// Refreshes the current folder. The periodic tick skips while a refresh is
    /// running; `force` (user action, panel opened) does not.
    pub(super) fn refresh_git(&mut self, force: bool) -> Task<Message> {
        if self.git.refreshing.is_some() && !force {
            return Task::none();
        }
        let dir = self.git_dir();
        // Same folder: reuse the known root (one git call instead of two).
        let root = (self.git.dir.as_ref() == Some(&dir))
            .then(|| self.git.root().cloned())
            .flatten();
        self.start_git_refresh(dir, root)
    }

    pub(super) fn start_git_refresh(
        &mut self,
        dir: PathBuf,
        known_root: Option<PathBuf>,
    ) -> Task<Message> {
        self.git.refreshing = Some(dir.clone());
        let with_log = self.panel_open(SidePanel::Git);
        let trusted: Vec<PathBuf> = self.git.trusted.iter().cloned().collect();
        background(
            move || git_snapshot(dir, known_root, with_log, &trusted),
            |s| Message::GitRefreshed(Box::new(s)),
        )
    }

    pub(super) fn git_action(&mut self, action: GitAction) -> Task<Message> {
        let Some(root) = self.git.root().cloned() else {
            return Task::none();
        };
        if self.git.action_running {
            return Task::none();
        }
        let has_commits = self.git.status().is_some_and(|s| s.branch.has_commits());
        self.git.action_running = true;
        self.git.last_action_was_commit = matches!(action, GitAction::Commit(_));
        self.git.last_action_was_create = matches!(action, GitAction::Create(_));
        background(
            move || match action {
                GitAction::Stage(paths) => forgeterm_git::stage(&root, &paths)
                    .map(|()| format!("Staged {}", describe(&paths))),
                GitAction::Unstage(paths) => forgeterm_git::unstage(&root, &paths, has_commits)
                    .map(|()| format!("Unstaged {}", describe(&paths))),
                GitAction::Commit(message) => {
                    forgeterm_git::commit(&root, &message).map(|s| format!("Committed {s}"))
                }
                GitAction::Switch(name) => {
                    forgeterm_git::switch(&root, &name).map(|()| format!("Switched to {name}"))
                }
                GitAction::Track(remote) => forgeterm_git::track(&root, &remote)
                    .map(|()| format!("Created and switched to a branch tracking {remote}")),
                GitAction::Create(name) => forgeterm_git::create_branch(&root, &name)
                    .map(|()| format!("Created and switched to {name}")),
            },
            Message::GitActionDone,
        )
    }
}
