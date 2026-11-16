//! Saving and restoring tabs, splits and viewer files.

use super::*;

impl App {
    pub(super) fn session_path(&self) -> Option<PathBuf> {
        self.settings_path
            .as_deref()
            .and_then(forgeterm_settings::session::path_for)
    }

    /// Reopens the last session's tabs, splits and viewer files when enabled and
    /// available; otherwise one tab with the default shell.
    pub(super) fn restore_or_new_tab(&mut self) -> Task<Message> {
        let saved = self
            .session_path()
            .filter(|_| self.settings.workspace.restore_session)
            .map(|p| forgeterm_settings::session::load(&p))
            .unwrap_or_default();
        let docked = saved.prompt_studio_docked;
        let task = self.restore_tabs(saved);
        if docked {
            // Prompt Studio was open beside the terminal: open it there again (empty;
            // typed values are never saved).
            self.prompt_docked = true;
            return Task::batch([task, self.open_prompt_studio()]);
        }
        task
    }

    pub(super) fn restore_tabs(
        &mut self,
        saved: forgeterm_settings::session::SavedSession,
    ) -> Task<Message> {
        // No shell at all: new_tab shows the error screen.
        if saved.tabs.is_empty() || self.shells.is_empty() {
            return self.new_tab(None, None);
        }
        for layout in &saved.tabs {
            let config = self.restore_layout(layout);
            let panes = pane_grid::State::with_configuration(config);
            let focus = panes.iter().next().map(|(p, _)| *p);
            if let Some(focus) = focus {
                self.workspace
                    .tabs
                    .push(crate::workspace::Tab { panes, focus });
            }
        }
        if self.workspace.tabs.is_empty() {
            return self.new_tab(None, None);
        }
        self.workspace.active = saved.active.min(self.workspace.tabs.len() - 1);
        let files: Vec<PathBuf> = saved
            .viewer_files
            .into_iter()
            .filter(|p| {
                forgeterm_terminal::is_local_path(&p.to_string_lossy())
                    && forgeterm_fs::resolves_locally(p)
                    && p.is_file()
            })
            .collect();
        if !files.is_empty() {
            self.viewer.visible = true;
        }
        Task::batch(files.into_iter().map(load_document))
    }

    pub(super) fn restore_layout(
        &mut self,
        layout: &forgeterm_settings::session::Layout,
    ) -> pane_grid::Configuration<TermId> {
        use forgeterm_settings::session::Layout;
        match layout {
            Layout::Pane(spec) => {
                // The saved shell if it is still installed, else the default one.
                let shell = self
                    .shells
                    .iter()
                    .find(|s| s.program.eq_ignore_ascii_case(&spec.shell))
                    .cloned()
                    .or_else(|| self.shell_at(None));
                let cwd = spec.cwd.clone().filter(|d| {
                    forgeterm_terminal::is_local_path(&d.to_string_lossy())
                        && forgeterm_fs::resolves_locally(d)
                        && d.is_dir()
                });
                let id = match shell {
                    Some(shell) => self.spawn_terminal(shell, cwd),
                    None => self.next_id,
                };
                pane_grid::Configuration::Pane(id)
            }
            Layout::Split {
                side_by_side,
                ratio,
                a,
                b,
            } => pane_grid::Configuration::Split {
                axis: if *side_by_side {
                    Axis::Vertical
                } else {
                    Axis::Horizontal
                },
                ratio: *ratio,
                a: Box::new(self.restore_layout(a)),
                b: Box::new(self.restore_layout(b)),
            },
        }
    }

    /// The current layout as a session (agents are saved as the default shell in
    /// their folder: an agent is never started automatically).
    pub(super) fn current_session(&self) -> forgeterm_settings::session::SavedSession {
        use forgeterm_settings::session::{Layout, PaneSpec, SavedSession};
        let default_program = self.shell_at(None).map(|s| s.program);
        let pane_spec = |id: &TermId| -> Option<PaneSpec> {
            let t = self.terminals.get(id)?;
            let shell = if t.agent.is_some() {
                default_program.clone()?
            } else {
                t.shell.program.clone()
            };
            Some(PaneSpec {
                shell,
                cwd: t.directory().map(Path::to_path_buf),
            })
        };
        fn walk(
            node: &pane_grid::Node,
            panes: &pane_grid::State<TermId>,
            spec: &dyn Fn(&TermId) -> Option<PaneSpec>,
        ) -> Option<Layout> {
            match node {
                pane_grid::Node::Pane(p) => panes.get(*p).and_then(spec).map(Layout::Pane),
                pane_grid::Node::Split {
                    axis, ratio, a, b, ..
                } => Some(Layout::Split {
                    side_by_side: *axis == Axis::Vertical,
                    ratio: *ratio,
                    a: Box::new(walk(a, panes, spec)?),
                    b: Box::new(walk(b, panes, spec)?),
                }),
            }
        }
        SavedSession {
            tabs: self
                .workspace
                .tabs
                .iter()
                .filter_map(|t| walk(t.panes.layout(), &t.panes, &pane_spec))
                .collect(),
            active: self.workspace.active,
            viewer_files: self
                .viewer
                .tabs
                .iter()
                .map(|t| t.doc.path.clone())
                .collect(),
            prompt_studio_docked: self.prompt_docked && self.prompt_studio.is_some(),
        }
    }

    /// Writes session.toml when it changed (and restoring is enabled).
    pub(super) fn save_session(&mut self) {
        if !self.settings.workspace.restore_session {
            return;
        }
        let Some(path) = self.session_path() else {
            return;
        };
        let session = self.current_session();
        let text = forgeterm_settings::session::to_toml(&session);
        if self.saved_session.as_deref() == Some(text.as_str()) {
            return;
        }
        match forgeterm_settings::session::save(&path, &session) {
            Ok(()) => {
                self.saved_session = Some(text);
                self.session_save_error = None;
            }
            Err(e) => {
                let e = e.to_string();
                // Saving is retried at every change: report a failure once, not
                // every 2 s.
                if self.session_save_error.as_ref() != Some(&e) {
                    tracing::warn!("could not save {}: {e}", path.display());
                    self.notice = Some(format!(
                        "Could not save the session to {}: {e}. Tabs and folders will not come back at the next start until this works.",
                        path.display()
                    ));
                    self.session_save_error = Some(e);
                }
            }
        }
    }
}
