//! What runs inside ForgeTerm's terminals, from the operating system's process
//! table (sysinfo). Read-only: nothing is ever signalled or killed from here.

mod usage;

pub use usage::{Sample, Sampler, TreeUsage};

use std::collections::HashMap;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// A process started from a terminal's shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub pid: u32,
    /// Executable name without `.exe` (`claude`, `vim`, `node`).
    pub name: String,
}

/// Console hosts belong to the terminal machinery, not to the user's work.
const IGNORED: [&str; 2] = ["conhost", "openconsole"];

/// For each shell pid, the programs it started directly (its child processes).
/// A shell that only waits at its prompt has none. Shells that are gone are
/// missing from the result.
pub fn running_programs(shells: &[u32]) -> HashMap<u32, Vec<Program>> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let mut out: HashMap<u32, Vec<Program>> = HashMap::new();
    for &shell in shells {
        if sys.process(Pid::from_u32(shell)).is_some() {
            out.insert(shell, Vec::new());
        }
    }
    for (pid, process) in sys.processes() {
        let Some(parent) = process.parent().map(Pid::as_u32) else {
            continue;
        };
        let Some(list) = out.get_mut(&parent) else {
            continue;
        };
        let name = clean_name(&process.name().to_string_lossy());
        if IGNORED.contains(&name.to_ascii_lowercase().as_str()) {
            continue;
        }
        list.push(Program {
            pid: pid.as_u32(),
            name,
        });
    }
    for list in out.values_mut() {
        list.sort_by_key(|p| p.pid);
    }
    out
}

fn clean_name(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    match lower.strip_suffix(".exe") {
        Some(_) => name[..name.len() - 4].to_owned(),
        None => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    #[test]
    fn strips_exe() {
        assert_eq!(clean_name("claude.exe"), "claude");
        assert_eq!(clean_name("PING.EXE"), "PING");
        assert_eq!(clean_name("vim"), "vim");
    }

    #[test]
    fn finds_the_program_a_shell_runs() {
        // A real "shell" that starts a long-running program.
        let mut shell = if cfg!(windows) {
            Command::new("cmd.exe")
                .args(["/d", "/c", "ping -n 30 127.0.0.1 >NUL"])
                .stdout(Stdio::null())
                .spawn()
                .unwrap()
        } else {
            Command::new("sh")
                .args(["-c", "sleep 30; true"])
                .spawn()
                .unwrap()
        };
        let pid = shell.id();
        let want = if cfg!(windows) { "ping" } else { "sleep" };
        // The child may take a moment to appear (virus scanners).
        let deadline = Instant::now() + Duration::from_secs(15);
        let found = loop {
            let progs = running_programs(&[pid]);
            let list = progs.get(&pid).cloned().unwrap_or_default();
            if list.iter().any(|p| p.name.eq_ignore_ascii_case(want)) || Instant::now() > deadline {
                break list;
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        // Clean up the program too, not only its shell (it would linger for 30 s).
        for p in &found {
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
        assert!(
            found.iter().any(|p| p.name.eq_ignore_ascii_case(want)),
            "{found:?}"
        );
        assert!(!found.iter().any(|p| p.name.eq_ignore_ascii_case("conhost")));
    }

    #[test]
    fn gone_shells_are_missing_and_idle_ones_are_empty() {
        let mut idle = if cfg!(windows) {
            // Waits for input that never comes: no child process.
            Command::new("cmd.exe")
                .args(["/d", "/k"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .spawn()
                .unwrap()
        } else {
            Command::new("sh").stdin(Stdio::piped()).spawn().unwrap()
        };
        std::thread::sleep(Duration::from_millis(500));
        let pid = idle.id();
        let progs = running_programs(&[pid, u32::MAX - 7]);
        let _ = idle.kill();
        let _ = idle.wait();
        assert_eq!(progs.get(&pid).map(Vec::len), Some(0), "{progs:?}");
        assert!(!progs.contains_key(&(u32::MAX - 7)));
    }
}
