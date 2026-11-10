//! Measured resource usage: the whole machine and each terminal's process tree.
//! Every number comes from the operating system via sysinfo; nothing is estimated.
//! CPU needs two measurements: the first sample has no CPU values.

use std::collections::{HashMap, HashSet};

use sysinfo::{
    CpuRefreshKind, MemoryRefreshKind, Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind,
    System,
};

/// Usage of one shell and everything it started (children, grandchildren, …).
#[derive(Debug, Clone, PartialEq)]
pub struct TreeUsage {
    /// Share of the whole machine (all cores = 100 %); `None` on the first sample.
    pub cpu_percent: Option<f32>,
    /// Resident memory of all processes in the tree, in bytes.
    pub memory: u64,
    pub processes: usize,
    /// Names of the programs below the shell (direct children first).
    pub programs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// All cores = 100 %; `None` on the first sample.
    pub cpu_percent: Option<f32>,
    pub memory_used: u64,
    pub memory_total: u64,
    pub logical_cpus: usize,
    /// Per shell pid; shells that are gone are missing.
    pub shells: HashMap<u32, TreeUsage>,
}

/// Keeps sysinfo's state between samples (CPU usage is a difference).
pub struct Sampler {
    sys: System,
    first: bool,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    pub fn new() -> Self {
        let sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
                .with_memory(MemoryRefreshKind::nothing().with_ram()),
        );
        Self { sys, first: true }
    }

    pub fn sample(&mut self, shells: &[u32]) -> Sample {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        let cpus = self.sys.cpus().len().max(1);
        let first = std::mem::take(&mut self.first);

        // Parent → children, once for all shells.
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for (pid, p) in self.sys.processes() {
            if let Some(parent) = p.parent() {
                children
                    .entry(parent.as_u32())
                    .or_default()
                    .push(pid.as_u32());
            }
        }
        let mut out = HashMap::new();
        for &shell in shells {
            let Some(root) = self.sys.process(Pid::from_u32(shell)) else {
                continue;
            };
            let mut cpu = root.cpu_usage();
            let mut memory = root.memory();
            let mut processes = 1;
            let mut programs = Vec::new();
            // Breadth-first: direct children come first in `programs`.
            let mut queue: Vec<u32> = children.get(&shell).cloned().unwrap_or_default();
            let mut seen: HashSet<u32> = HashSet::from([shell]);
            let mut i = 0;
            while i < queue.len() {
                let pid = queue[i];
                i += 1;
                if !seen.insert(pid) {
                    continue;
                }
                let Some(p) = self.sys.process(Pid::from_u32(pid)) else {
                    continue;
                };
                cpu += p.cpu_usage();
                memory += p.memory();
                processes += 1;
                let name = crate::clean_name(&p.name().to_string_lossy());
                if !programs.contains(&name) {
                    programs.push(name);
                }
                if let Some(more) = children.get(&pid) {
                    queue.extend(more);
                }
            }
            out.insert(
                shell,
                TreeUsage {
                    // sysinfo reports per process in "% of one core".
                    cpu_percent: (!first).then(|| (cpu / cpus as f32).clamp(0.0, 100.0)),
                    memory,
                    processes,
                    programs,
                },
            );
        }
        Sample {
            cpu_percent: (!first).then(|| self.sys.global_cpu_usage().clamp(0.0, 100.0)),
            memory_used: self.sys.used_memory(),
            memory_total: self.sys.total_memory(),
            logical_cpus: cpus,
            shells: out,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::Duration;

    #[test]
    fn measures_a_real_process_tree() {
        // A shell with one long-running child.
        let mut shell = if cfg!(windows) {
            Command::new("cmd.exe")
                .args(["/d", "/c", "ping -n 20 127.0.0.1 >NUL"])
                .stdout(Stdio::null())
                .spawn()
                .unwrap()
        } else {
            Command::new("sh")
                .args(["-c", "sleep 20; true"])
                .spawn()
                .unwrap()
        };
        let pid = shell.id();
        std::thread::sleep(Duration::from_millis(800));
        let mut sampler = Sampler::new();
        let first = sampler.sample(&[pid, u32::MAX - 3]);
        assert_eq!(
            first.cpu_percent, None,
            "no CPU value without a previous sample"
        );
        assert!(first.memory_total > 0 && first.memory_used > 0);
        assert!(first.memory_used <= first.memory_total);
        assert!(first.logical_cpus >= 1);
        let tree = &first.shells[&pid];
        assert_eq!(tree.cpu_percent, None);
        assert!(
            !first.shells.contains_key(&(u32::MAX - 3)),
            "gone shell missing"
        );

        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL + Duration::from_millis(300));
        let second = sampler.sample(&[pid]);
        let cpu = second.cpu_percent.expect("CPU value on the second sample");
        assert!((0.0..=100.0).contains(&cpu));
        let tree = &second.shells[&pid];
        let tree_cpu = tree.cpu_percent.unwrap();
        assert!((0.0..=100.0).contains(&tree_cpu));
        assert!(tree.processes >= 2, "shell + child: {tree:?}");
        assert!(tree.memory > 0);
        let want = if cfg!(windows) { "ping" } else { "sleep" };
        assert!(
            tree.programs.iter().any(|p| p.eq_ignore_ascii_case(want)),
            "{tree:?}"
        );

        // Clean up our child (by its own pid only, never by name) and the shell.
        for p in crate::running_programs(&[pid])
            .remove(&pid)
            .unwrap_or_default()
        {
            let _ = if cfg!(windows) {
                Command::new("taskkill")
                    .args(["/PID", &p.pid.to_string(), "/F"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
            } else {
                Command::new("kill").arg(p.pid.to_string()).status()
            };
        }
        let _ = shell.kill();
        let _ = shell.wait();
    }
}
