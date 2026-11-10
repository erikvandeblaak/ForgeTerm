//! End-to-end: every installed shell, started with ForgeTerm's shell integration in a
//! real PTY, reports its working directory after `cd`, including awkward names.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use forgeterm_terminal::{
    GridSize, Session, SessionConfig, SessionEvent, ShellKind, detect_shells, quote_path,
};

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Waits until the session reports `want` as its directory; returns all reports seen.
fn wait_for_cwd(
    rx: &mpsc::Receiver<SessionEvent>,
    want: &Path,
) -> Result<Vec<String>, Vec<String>> {
    let deadline = Instant::now() + Duration::from_secs(25);
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        if let Ok(SessionEvent::Cwd(p)) = rx.recv_timeout(Duration::from_millis(200)) {
            let hit = same_dir(Path::new(&p), want);
            seen.push(p);
            if hit {
                return Ok(seen);
            }
        }
    }
    Err(seen)
}

#[test]
fn installed_shells_report_cwd_after_cd() {
    let base = std::env::temp_dir().join(format!("forgeterm cwd {}", std::process::id()));
    let start = base.join("start");
    let target: PathBuf = base.join("it's a dir");
    std::fs::create_dir_all(&start).unwrap();
    std::fs::create_dir_all(&target).unwrap();

    let mut verified = Vec::new();
    for shell in detect_shells() {
        // WSL does not inherit the integration; zsh needs a user hook (documented).
        if shell.kind == ShellKind::Wsl {
            continue;
        }
        let (tx, rx) = mpsc::channel();
        let config = SessionConfig {
            shell: Some(shell.clone()),
            working_directory: Some(start.clone()),
            size: GridSize::new(120, 30, 8, 16),
            ..Default::default()
        };
        let session = Session::spawn(config, move |e| {
            let _ = tx.send(e);
        })
        .expect("spawn shell");

        // The first prompt reports the start directory.
        if let Err(seen) = wait_for_cwd(&rx, &start) {
            panic!("{}: no report of the start dir; saw {seen:?}", shell.name);
        }

        let cd = format!("cd {}\r", quote_path(shell.kind, &target.to_string_lossy()));
        session.input(cd.into_bytes());
        match wait_for_cwd(&rx, &target) {
            Ok(_) => verified.push(shell.name.clone()),
            Err(seen) => panic!("{}: cd not reported; saw {seen:?}", shell.name),
        }
        session.input(b"exit\r".to_vec());
    }
    let _ = std::fs::remove_dir_all(&base);
    println!("cwd tracking verified in: {verified:?}");
}
