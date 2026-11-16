//! Mission Control: agent cards, heads, CPU and ended sessions.

use super::*;

impl App {
    /// Mission Control or the Git Control Center covers the terminals.
    pub(super) fn main_covered(&self) -> bool {
        self.mission.visible || self.git_dash.visible || self.compare.visible
    }

    pub(super) fn toggle_mission(&mut self) -> Task<Message> {
        self.mission.visible = !self.mission.visible;
        if self.mission.visible {
            self.git_dash.visible = false;
            self.compare.visible = false;
        }
        if !self.mission.visible {
            self.refresh_visible();
            return Task::none();
        }
        // Samples are only taken while it is open: an old history would join
        // measurements minutes apart as if they were consecutive.
        self.mission.cpu.clear();
        // Fresh previews right away (hidden terminals only mark themselves dirty).
        for t in self.terminals.values_mut().filter(|t| t.agent.is_some()) {
            t.refresh(&self.palette);
        }
        let order = self.mission_order();
        if !self.mission.selected.is_some_and(|id| order.contains(&id)) {
            self.mission.selected = order.first().copied();
        }
        Task::batch(
            [self.read_mission_heads()]
                .into_iter()
                .chain(self.sample_monitor()),
        )
    }

    pub(super) fn go_to_terminal(&mut self, id: TermId) {
        if !self.terminals.contains_key(&id) {
            return;
        }
        // Whichever page covers the terminals (Mission Control, Git Control Center).
        self.mission.visible = false;
        self.git_dash.visible = false;
        self.compare.visible = false;
        self.mission.selected = Some(id);
        self.focus_terminal(id);
        self.refresh_visible();
    }

    /// Agent terminals in dashboard order: most urgent first, then tab order.
    pub(super) fn mission_order(&self) -> Vec<TermId> {
        let now = std::time::Instant::now();
        self.mission_cards(now).iter().map(|c| c.id).collect()
    }

    pub(super) fn mission_cards(&self, now: std::time::Instant) -> Vec<mission_view::Card<'_>> {
        use forgeterm_agents::mission::{Reported, classify};
        use forgeterm_terminal::status::ProgramState;
        let focused = self.workspace.focused();
        let usage = self.monitor.last.as_ref();
        let mut cards = Vec::new();
        for (tab, t_ids) in self
            .workspace
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| (i, tab.terminals().collect::<Vec<_>>()))
        {
            for id in t_ids {
                let Some(t) = self.terminals.get(&id) else {
                    continue;
                };
                let (Some(kind), Some(root)) = (t.agent, t.directory()) else {
                    continue;
                };
                let record = t.program_status();
                let reported = record.as_ref().map(|r| match r.state {
                    ProgramState::Working => Reported::Working,
                    ProgramState::Blocked => Reported::Blocked,
                    ProgramState::Idle => Reported::Idle,
                    ProgramState::Done => Reported::Done,
                    ProgramState::Error => Reported::Error,
                });
                let message = record.and_then(|r| r.msg.or(r.title));
                let pid = t.session().and_then(|s| s.pid());
                cards.push(mission_view::Card {
                    id,
                    kind,
                    tab,
                    title: t.display_title(),
                    root,
                    head: self.mission.heads.get(root).and_then(Option::as_ref),
                    status: classify(&t.process(), reported, t.bell),
                    message,
                    elapsed: t.elapsed(now),
                    since_output: t.since_output(now),
                    usage: pid.and_then(|p| usage?.shells.get(&p)),
                    cpu: self.mission.cpu.get(&id).map(|(_, h)| h),
                    snapshot: &t.snapshot,
                    cache: &t.preview_cache,
                    selected: self.mission.selected == Some(id),
                    focused: focused == Some(id),
                });
            }
        }
        // Stable: equal urgency keeps tab order.
        cards.sort_by_key(|c| c.status.urgency());
        cards
    }

    pub(super) fn view_mission(&self) -> Element<'_, Message> {
        let now = std::time::Instant::now();
        let start_root = self.ai_context.as_ref().map(|c| c.root.as_path());
        mission_view::view(mission_view::View {
            cards: self.mission_cards(now),
            mission: &self.mission,
            agents: &self.agents,
            start_root,
            now,
            worktree_ok: self.is_applicable(CommandId::NewAgentWorktree),
            back_hint: self.keymap.hint(CommandId::ToggleMissionControl),
        })
    }

    /// Reads the branch of every agent's project in the background (no git process).
    pub(super) fn read_mission_heads(&mut self) -> Task<Message> {
        if self.mission.heads_busy {
            return Task::none();
        }
        let mut roots: Vec<PathBuf> = self
            .terminals
            .values()
            .filter(|t| t.agent.is_some())
            .filter_map(|t| t.directory())
            .filter(|d| d.to_str().is_some_and(forgeterm_terminal::is_local_path))
            .map(Path::to_path_buf)
            .collect();
        roots.sort();
        roots.dedup();
        if roots.is_empty() {
            return Task::none();
        }
        self.mission.heads_busy = true;
        background(
            move || {
                roots
                    .into_iter()
                    .map(|r| {
                        let head = forgeterm_git::head(&r);
                        (r, head)
                    })
                    .collect()
            },
            Message::MissionHeads,
        )
    }

    pub(super) fn record_mission_cpu(&mut self, sample: &forgeterm_system::Sample) {
        let agents: Vec<(TermId, u32)> = self
            .terminals
            .values()
            .filter(|t| t.agent.is_some())
            .filter_map(|t| Some((t.id, t.session()?.pid()?)))
            .collect();
        for &(id, pid) in &agents {
            if let Some(cpu) = sample.shells.get(&pid).and_then(|u| u.cpu_percent) {
                self.mission.record_cpu(id, pid, cpu);
            }
        }
        // Ended or closed agents keep no (frozen) history.
        self.mission
            .cpu
            .retain(|id, _| agents.iter().any(|(a, _)| a == id));
    }

    /// Remembers an agent session that is going away, for "recently ended".
    pub(super) fn note_ended(&mut self, id: TermId) {
        let Some(t) = self.terminals.get(&id) else {
            return;
        };
        let Some(kind) = t.agent else {
            return;
        };
        let how = match &t.status {
            Status::Running => EndKind::Closed,
            Status::Exited(code) => EndKind::Exited(*code),
            Status::Failed(_) => EndKind::FailedToStart,
        };
        let now = std::time::Instant::now();
        let project = t
            .directory()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.mission.record_ended(Ended {
            kind,
            project,
            ran: t.elapsed(now),
            at: now,
            how,
        });
        if self.mission.selected == Some(id) {
            self.mission.selected = None;
        }
    }

    /// Terminals the user is looking at no longer need attention.
    pub(super) fn acknowledge_seen(&mut self) {
        if !self.window_focused || self.main_covered() {
            return;
        }
        let Some(tab) = self.workspace.active_tab() else {
            return;
        };
        for id in tab.terminals().collect::<Vec<_>>() {
            if let Some(t) = self.terminals.get_mut(&id) {
                t.acknowledge();
            }
        }
    }
}
