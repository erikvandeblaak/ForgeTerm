//! Finding agent executables and their versions.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crate::AgentKind;

/// A detected agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub kind: AgentKind,
    pub path: PathBuf,
    /// First line of `--version`, or why it could not be determined.
    pub version: Result<String, String>,
}

/// Extensions tried on Windows, in order. `.ps1` is skipped on purpose: it needs
/// PowerShell and an execution policy; the `.cmd` shim next to it does the same.
const WINDOWS_EXTENSIONS: [&str; 3] = ["exe", "cmd", "bat"];

/// Finds `kind`'s executable on `path_var` (the value of `PATH`).
pub fn locate_in(kind: AgentKind, path_var: Option<OsString>) -> Option<PathBuf> {
    let dirs: Vec<PathBuf> = env::split_paths(&path_var?).collect();
    for dir in dirs {
        if cfg!(windows) {
            for ext in WINDOWS_EXTENSIONS {
                let candidate = dir.join(format!("{}.{ext}", kind.program()));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        } else {
            let candidate = dir.join(kind.program());
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Finds `kind`'s executable on this process' `PATH`, then in the documented
/// location of Claude Code's native installer (`~/.local/bin`), which a freshly
/// started program may not have on its `PATH` yet.
pub fn locate(kind: AgentKind) -> Option<PathBuf> {
    locate_in(kind, env::var_os("PATH")).or_else(|| {
        let home = env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
        let local_bin = PathBuf::from(home).join(".local").join("bin");
        locate_in(kind, Some(local_bin.into_os_string()))
    })
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

/// How to start an executable in a PTY. Windows batch shims (`.cmd`, as installed
/// by npm) cannot be started directly and go through `cmd.exe /d /c`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchCommand {
    pub program: String,
    pub args: Vec<String>,
}

pub fn launch_command(path: &Path) -> LaunchCommand {
    let is_batch = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    let path_str = path.to_string_lossy().into_owned();
    if is_batch {
        LaunchCommand {
            program: "cmd.exe".into(),
            // /d: no AutoRun commands from the registry.
            args: vec!["/d".into(), "/c".into(), path_str],
        }
    } else {
        LaunchCommand {
            program: path_str,
            args: Vec::new(),
        }
    }
}

/// Runs `<path> --version` (stdin closed, output captured) and returns its first
/// non-empty line. Gives up after `timeout` (the child is killed).
pub fn query_version(path: &Path, timeout: Duration) -> Result<String, String> {
    let mut cmd = Command::new(path);
    // stderr is not read: discard it, a full pipe would block the child.
    cmd.arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    hide_console(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("could not run: {e}"))?;
    let stdout = child.stdout.take();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut out = String::new();
        if let Some(mut s) = stdout {
            let _ = std::io::Read::read_to_string(&mut s, &mut out);
        }
        let _ = tx.send(out);
    });
    match rx.recv_timeout(timeout) {
        Ok(out) => {
            let status = child.wait().map_err(|e| e.to_string())?;
            let line = out
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .map(str::to_owned);
            match (status.success(), line) {
                (true, Some(line)) => Ok(line),
                (true, None) => Err("no version output".into()),
                (false, _) => Err(format!("--version failed ({status})")),
            }
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(format!("no answer within {} s", timeout.as_secs()))
        }
    }
}

/// No console window flashes up (ForgeTerm has none in Windows release builds).
#[cfg(windows)]
fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(tag: &str) -> PathBuf {
        let d = env::temp_dir().join(format!("forgeterm-agents-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn missing_agent_is_none() {
        let dir = temp("none");
        assert_eq!(
            locate_in(AgentKind::Codex, Some(dir.clone().into_os_string())),
            None
        );
        assert_eq!(locate_in(AgentKind::Codex, None), None);
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(windows)]
    #[test]
    fn windows_prefers_exe_and_skips_ps1() {
        let a = temp("a");
        let b = temp("b");
        fs::write(a.join("claude.ps1"), "").unwrap();
        fs::write(b.join("claude.cmd"), "").unwrap();
        fs::write(b.join("claude.exe"), "").unwrap();
        let path = env::join_paths([&a, &b]).unwrap();
        assert_eq!(
            locate_in(AgentKind::ClaudeCode, Some(path)),
            Some(b.join("claude.exe"))
        );
        fs::remove_file(b.join("claude.exe")).unwrap();
        let path = env::join_paths([&a, &b]).unwrap();
        assert_eq!(
            locate_in(AgentKind::ClaudeCode, Some(path)),
            Some(b.join("claude.cmd"))
        );
        let _ = fs::remove_dir_all(a);
        let _ = fs::remove_dir_all(b);
    }

    #[test]
    fn batch_shims_go_through_cmd() {
        let l = launch_command(Path::new(r"C:\Program Files\nodejs\claude.cmd"));
        assert_eq!(l.program, "cmd.exe");
        assert_eq!(l.args, ["/d", "/c", r"C:\Program Files\nodejs\claude.cmd"]);
        let l = launch_command(Path::new("/usr/local/bin/claude"));
        assert_eq!(l.program, "/usr/local/bin/claude");
        assert!(l.args.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn version_of_a_real_program_and_a_failing_one() {
        // cmd's `ver` prints "Microsoft Windows [Version …]" after an empty line.
        let dir = temp("ver");
        let ok = dir.join("fake.cmd");
        fs::write(&ok, "@echo off\r\necho.\r\necho 9.9.9 (Fake Agent)\r\n").unwrap();
        assert_eq!(
            query_version(&ok, Duration::from_secs(20)).as_deref(),
            Ok("9.9.9 (Fake Agent)")
        );
        let bad = dir.join("bad.cmd");
        fs::write(&bad, "@echo off\r\nexit /b 3\r\n").unwrap();
        assert!(query_version(&bad, Duration::from_secs(20)).is_err());
        let slow = dir.join("slow.cmd");
        fs::write(&slow, "@echo off\r\nping -n 30 127.0.0.1 >nul\r\n").unwrap();
        let err = query_version(&slow, Duration::from_secs(1)).unwrap_err();
        assert!(err.contains("no answer"), "{err}");
        let _ = fs::remove_dir_all(dir);
    }
}
