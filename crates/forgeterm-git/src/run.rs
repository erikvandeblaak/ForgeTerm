//! Running `git` safely: fixed safety options, no prompts, timeout, both output
//! streams drained (a full stderr pipe can otherwise block the child forever).

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// `None` when git was ended by a signal.
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl Output {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    /// git's own message (first lines of stderr), for the UI.
    pub fn error_text(&self) -> String {
        let text: Vec<&str> = self
            .stderr
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .take(4)
            .collect();
        if text.is_empty() {
            match self.code {
                Some(code) => format!("git failed (exit code {code})"),
                None => "git was interrupted".to_owned(),
            }
        } else {
            text.join("\n")
        }
    }

    pub fn check(&self) -> Result<(), String> {
        if self.success() {
            Ok(())
        } else {
            Err(self.error_text())
        }
    }
}

/// Options in front of every command (see the crate docs).
const SAFETY: [&str; 7] = [
    "--no-optional-locks",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "color.ui=false",
    "-c",
    "core.pager=cat",
];

/// Runs `git <safety options> <args>` in `dir`. `Err` only when git could not be
/// started or timed out; a failing git command is an `Ok` with its exit code.
pub fn git(dir: &Path, args: &[&str], timeout: Duration) -> Result<Output, String> {
    let mut cmd = Command::new("git");
    cmd.args(SAFETY)
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "git is not installed (or not on PATH)".to_owned()
        } else {
            format!("could not run git: {e}")
        }
    })?;

    let drain = |stream: Option<Box<dyn Read + Send>>| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut s) = stream {
                let _ = s.read_to_end(&mut buf);
            }
            let _ = tx.send(buf);
        });
        rx
    };
    let out_rx = drain(
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let err_rx = drain(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );

    // stdout closes when git exits (or its children do); wait for it with a limit.
    let stdout = match out_rx.recv_timeout(timeout) {
        Ok(buf) => buf,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "git {} gave no answer within {} s",
                args.first().copied().unwrap_or_default(),
                timeout.as_secs()
            ));
        }
    };
    let stderr = err_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or_default();
    let status = child.wait().map_err(|e| e.to_string())?;
    Ok(Output {
        code: status.code(),
        stdout,
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

/// No console window flashes up for each git call (ForgeTerm itself has none in
/// release builds on Windows).
#[cfg(windows)]
fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_: &mut Command) {}
