//! Compare Worktrees.

use super::*;

impl App {
    /// Reads the Compare page for the shown repository (background).
    pub(super) fn load_compare(&mut self) -> Task<Message> {
        let Some(Repo::Found { root, .. }) = self.git.repo.as_ref() else {
            return Task::none();
        };
        self.compare.seq += 1;
        self.compare.busy = true;
        let (seq, root) = (self.compare.seq, root.clone());
        let (a, b) = (self.compare.a.clone(), self.compare.b.clone());
        let trusted: Vec<PathBuf> = self.git.trusted.iter().cloned().collect();
        background(
            move || Box::new(compare_view::load(seq, root, a, b, &trusted)),
            Message::CompareLoaded,
        )
    }

    /// Other worktrees chosen: the file shown belonged to the old pair.
    pub(super) fn compare_sides_changed(&mut self) -> Task<Message> {
        self.compare.selected = None;
        self.compare.diff = None;
        self.load_compare()
    }

    /// Side-by-side diff of `path` between A and B. `reset`: a new selection
    /// (show "Reading…"); otherwise the old rows stay until the new ones arrive.
    pub(super) fn load_compare_diff(&mut self, path: String, reset: bool) -> Task<Message> {
        let Some(data) = &self.compare.data else {
            return Task::none();
        };
        let Some([a, b]) = self.compare.paths() else {
            return Task::none();
        };
        let root = data.root.clone();
        self.compare.selected = Some(path.clone());
        if reset || self.compare.diff.as_ref().is_none_or(|d| d.path != path) {
            self.compare.diff = Some(compare_view::DiffState {
                path: path.clone(),
                result: None,
            });
        }
        background(
            move || {
                let r = compare_view::load_diff(&root, &a, &b, &path);
                ([a, b], path, r)
            },
            |(pair, path, r)| Message::CompareDiffLoaded(pair, path, r),
        )
    }

    /// Agents of this window per side (deepest worktree of their folder).
    pub(super) fn compare_agents(&self) -> [Vec<compare_view::AgentIn>; 2] {
        let mut out: [Vec<compare_view::AgentIn>; 2] = Default::default();
        let Some(paths) = self.compare.paths() else {
            return out;
        };
        for (tab, t) in self.workspace.tabs.iter().enumerate() {
            for id in t.terminals() {
                if let Some(term) = self.terminals.get(&id)
                    && let (Some(kind), Some(dir)) = (term.agent, term.directory())
                    && let Some(side) = git_dash::worktree_of(dir, &paths)
                {
                    out[side].push(compare_view::AgentIn { id, kind, tab });
                }
            }
        }
        out
    }

    /// A tab with a shell in A beside one in B; the test command is typed into each
    /// once its prompt shows, never followed by Enter.
    pub(super) fn open_compare_tests(&mut self) -> Task<Message> {
        let Some([a, b]) = self.compare.paths() else {
            return Task::none();
        };
        let command = self.compare.test_command.trim().to_owned();
        if command.is_empty() {
            return Task::none();
        }
        if let Some(p) = [&a, &b].into_iter().find(|p| {
            !p.to_str().is_some_and(forgeterm_terminal::is_local_path)
                || !forgeterm_fs::resolves_locally(p)
                || !p.is_dir()
        }) {
            self.notice = Some(format!(
                "No test terminals: {} is not a local folder.",
                p.display()
            ));
            return Task::none();
        }
        let Some(shell) = self.shell_at(None) else {
            self.error = Some("No supported shell was found on this system.".into());
            return Task::none();
        };
        self.compare.visible = false;
        let ia = self.spawn_terminal(shell.clone(), Some(a));
        self.workspace.push_tab(ia);
        let new_b = self.spawn_terminal(shell, Some(b));
        let split = self
            .workspace
            .active_tab_mut()
            .and_then(|tab| tab.split(Axis::Vertical, new_b));
        let ib = if split.is_some() {
            Some(new_b)
        } else {
            self.terminals.remove(&new_b);
            None
        };
        let since = std::time::Instant::now();
        for id in std::iter::once(ia).chain(ib) {
            self.pending_inserts.push(PendingInsert {
                id,
                text: command.clone(),
                since,
            });
        }
        self.compare.test_terms = [Some(ia), ib];
        self.focus_terminal(ia);
        self.after_layout_change()
    }

    /// Types waiting test commands into shells that are ready (no Enter).
    pub(super) fn process_pending_inserts(&mut self) {
        let now = std::time::Instant::now();
        for p in std::mem::take(&mut self.pending_inserts) {
            let Some(t) = self.terminals.get(&p.id) else {
                continue;
            };
            let Some(s) = t.session() else {
                // Not started yet (it starts at its first layout): keep waiting.
                if t.is_awaiting_size()
                    && now.duration_since(p.since) <= compare_view::INSERT_GIVE_UP
                {
                    self.pending_inserts.push(p);
                }
                continue;
            };
            if compare_view::ready_for_insert(s.at_prompt(), t.since_output(now)) {
                self.insert_text_into(p.id, p.text);
            } else if now.duration_since(p.since) > compare_view::INSERT_GIVE_UP {
                self.notice = Some(format!(
                    "{} showed no prompt within {} s: type the test command yourself.",
                    t.display_title(),
                    compare_view::INSERT_GIVE_UP.as_secs()
                ));
            } else {
                self.pending_inserts.push(p);
            }
        }
    }

    /// The Compare prompt at the first agent of each side, like a paste, no Enter.
    pub(super) fn insert_compare_prompt(&mut self) {
        let raw = self.compare.prompt.text();
        let text = raw.trim_end_matches(['\n', '\r']).to_owned();
        if text.trim().is_empty() {
            return;
        }
        let agents = self.compare_agents();
        let (mut done, mut skipped) = (Vec::new(), Vec::new());
        for (label, list) in [("A", &agents[0]), ("B", &agents[1])] {
            let Some(ag) = list.first() else {
                skipped.push(format!("{label} has no agent"));
                continue;
            };
            let Some(t) = self.terminals.get(&ag.id) else {
                continue;
            };
            let name = format!("{} (tab {})", ag.kind.name(), ag.tab + 1);
            match t.session() {
                Some(_) if paste_needs_confirmation(&text, t.modes.bracketed_paste) => {
                    skipped.push(format!(
                        "{name} has not enabled bracketed paste, so the lines would run one by one"
                    ));
                }
                Some(s) => {
                    s.paste(&text);
                    done.push(name);
                }
                None => skipped.push(format!("{name} has ended")),
            }
        }
        let mut notice = if done.is_empty() {
            "Nothing inserted.".to_owned()
        } else {
            format!(
                "Inserted into {}, without Enter: read it there and press Enter yourself.",
                done.join(" and ")
            )
        };
        if !skipped.is_empty() {
            notice.push_str(&format!(" Not inserted: {}.", skipped.join("; ")));
        }
        self.notice = Some(notice);
    }

    /// What a test terminal shows, from its shell's command marks.
    pub(super) fn compare_test_state(
        &self,
        id: Option<TermId>,
    ) -> (Option<TermId>, compare_view::TestState) {
        use compare_view::TestState;
        use forgeterm_terminal::marks::CommandState;
        let Some((id, t)) = id.and_then(|id| self.terminals.get(&id).map(|t| (id, t))) else {
            return (None, TestState::None);
        };
        if self.pending_inserts.iter().any(|p| p.id == id) {
            return (Some(id), TestState::Inserting);
        }
        let Some(s) = t.session() else {
            return (Some(id), TestState::Ended);
        };
        let state = match s.last_run() {
            Some(r) => match r.state {
                CommandState::Running => TestState::Running(r.command),
                CommandState::Finished(code) => TestState::Finished(r.command, code),
                CommandState::AtPrompt => TestState::NothingYet,
            },
            // cmd marks prompts but not commands: it never reports an exit code.
            None if s.at_prompt() && t.shell.kind != forgeterm_terminal::ShellKind::Cmd => {
                TestState::NothingYet
            }
            None => TestState::NotReported,
        };
        (Some(id), state)
    }

    pub(super) fn view_compare(&self) -> Element<'_, Message> {
        compare_view::view(compare_view::View {
            repo: self.git.repo.as_ref(),
            compare: &self.compare,
            agents: self.compare_agents(),
            tests: [
                self.compare_test_state(self.compare.test_terms[0]),
                self.compare_test_state(self.compare.test_terms[1]),
            ],
            now: std::time::Instant::now(),
        })
    }
}
