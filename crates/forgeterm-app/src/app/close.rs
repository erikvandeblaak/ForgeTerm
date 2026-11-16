//! Closing tabs, panes and the window (asks when programs run).

use super::*;

impl App {
    /// Closes at once when nothing runs in the affected terminals; otherwise asks
    /// first, naming the programs (looked up off the UI thread).
    pub(super) fn request_close(&mut self, target: CloseTarget) -> Task<Message> {
        let ids: Vec<TermId> = match &target {
            CloseTarget::Quit => self.terminals.keys().copied().collect(),
            CloseTarget::Tab(ids) => ids.clone(),
            CloseTarget::Pane(id) => vec![*id],
        };
        let shells: Vec<(u32, String)> = ids
            .iter()
            .filter_map(|id| {
                let t = self.terminals.get(id)?;
                let pid = t.session()?.pid()?;
                let tab = self.workspace.locate(*id).map_or(0, |(i, _)| i + 1);
                Some((pid, format!("{} (tab {tab})", t.display_title())))
            })
            .collect();
        // An agent runs as the terminal's own process (no child of a shell), so the
        // process check would not see it: ending a session always asks.
        let agents: Vec<String> = ids
            .iter()
            .filter_map(|id| {
                let t = self.terminals.get(id).filter(|t| t.is_running())?;
                let kind = t.agent?;
                let tab = self.workspace.locate(*id).map_or(0, |(i, _)| i + 1);
                Some(format!("{kind} session (tab {tab})"))
            })
            .collect();
        if shells.is_empty() && agents.is_empty() {
            return self.perform_close(target);
        }
        background(
            move || {
                let pids: Vec<u32> = shells.iter().map(|(p, _)| *p).collect();
                let running = forgeterm_system::running_programs(&pids);
                let mut found = agents;
                for (pid, label) in &shells {
                    if let Some(programs) = running.get(pid).filter(|p| !p.is_empty()) {
                        let names: Vec<&str> = programs.iter().map(|p| p.name.as_str()).collect();
                        found.push(format!("{} in {label}", names.join(", ")));
                    }
                }
                (target, found)
            },
            |(target, found)| Message::CloseChecked(target, found),
        )
    }

    pub(super) fn perform_close(&mut self, target: CloseTarget) -> Task<Message> {
        match target {
            // Same as closing the window: sessions are dropped, shells are shut down.
            CloseTarget::Quit => {
                self.save_session();
                iced::exit()
            }
            CloseTarget::Tab(ids) => {
                let index = ids
                    .first()
                    .and_then(|id| self.workspace.locate(*id))
                    .map(|(i, _)| i);
                match index {
                    Some(i) => self.close_tab(i),
                    None => Task::none(),
                }
            }
            CloseTarget::Pane(id) => self.close_terminal(id),
        }
    }
}
