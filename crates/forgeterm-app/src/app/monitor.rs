//! The monitor panel samples.

use super::*;

impl App {
    /// (label, shell pid) of every running terminal, in tab order.
    pub(super) fn monitor_rows(&self) -> Vec<(String, u32)> {
        let mut rows = Vec::new();
        for (i, tab) in self.workspace.tabs.iter().enumerate() {
            for id in tab.terminals() {
                if let Some(t) = self.terminals.get(&id)
                    && let Some(pid) = t.session().and_then(|s| s.pid())
                {
                    rows.push((format!("{} (tab {})", t.display_title(), i + 1), pid));
                }
            }
        }
        rows
    }

    /// Starts a measurement when the monitor is open and none is running.
    pub(super) fn sample_monitor(&mut self) -> Option<Task<Message>> {
        if !(self.panel_open(SidePanel::Monitor) || self.mission.visible) || self.monitor.busy {
            return None;
        }
        self.monitor.busy = true;
        let pids: Vec<u32> = self.monitor_rows().into_iter().map(|(_, p)| p).collect();
        let sampler = self.monitor.sampler.clone();
        Some(background(
            move || {
                let mut s = sampler
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                Box::new(s.sample(&pids))
            },
            Message::MonitorSampled,
        ))
    }
}
