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
        // Panel closed: let go of the GPU driver (a running sample holds the lock and
        // drops it itself when the GPU is not wanted).
        if !self.panel_open(SidePanel::Monitor)
            && let Ok(mut probe) = self.monitor.gpu_probe.try_lock()
        {
            *probe = None;
        }
        if !(self.panel_open(SidePanel::Monitor) || self.mission.visible) || self.monitor.busy {
            return None;
        }
        self.monitor.busy = true;
        let pids: Vec<u32> = self.monitor_rows().into_iter().map(|(_, p)| p).collect();
        let sampler = self.monitor.sampler.clone();
        // Mission Control samples too, for its CPU figures; the GPU only for the panel.
        let want_gpu = self.panel_open(SidePanel::Monitor);
        let gpu_probe = self.monitor.gpu_probe.clone();
        Some(background(
            move || {
                let mut sample = sampler
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .sample(&pids);
                let mut probe = gpu_probe
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if want_gpu {
                    sample.gpu = Some(
                        probe
                            .get_or_insert_with(forgeterm_system::GpuProbe::open)
                            .sample(),
                    );
                } else {
                    // Closing the driver connection lets a laptop GPU sleep again.
                    *probe = None;
                }
                Box::new(sample)
            },
            Message::MonitorSampled,
        ))
    }
}
