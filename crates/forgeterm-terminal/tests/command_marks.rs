//! End-to-end: every installed shell with ForgeTerm's shell integration, in a real
//! PTY, marks its prompts in the grid (so commands can be navigated), and shells that
//! report commands give the last command's output and exit code. Also checks the
//! program-status protocol (OSC 7501): the capability query is answered (Unix; see
//! below for Windows) and reports are kept.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use forgeterm_terminal::status::ProgramState;
use forgeterm_terminal::{
    GridSize, Palette, PromptMark, Session, SessionConfig, SessionEvent, ShellKind, ShellSpec,
    detect_shells,
};

fn spawn(shell: &ShellSpec) -> (Session, mpsc::Receiver<SessionEvent>) {
    let (tx, rx) = mpsc::channel();
    let config = SessionConfig {
        shell: Some(shell.clone()),
        working_directory: Some(std::env::temp_dir()),
        size: GridSize::new(100, 30, 8, 16),
        ..Default::default()
    };
    let session = Session::spawn(config, move |e| {
        let _ = tx.send(e);
    })
    .expect("spawn shell");
    (session, rx)
}

/// Polls `check` until it holds (or 30 s pass); drains events meanwhile.
fn wait(rx: &mpsc::Receiver<SessionEvent>, what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if check() {
            return;
        }
        let _ = rx.recv_timeout(Duration::from_millis(100));
    }
    panic!("timed out waiting for: {what}");
}

#[cfg(unix)]
fn screen(session: &Session) -> String {
    session.snapshot(&Palette::default()).text()
}

/// (failing command with output `forge-out-1` and exit code 3 where the shell can
/// report it, command printing 100 lines)
fn commands(kind: ShellKind) -> (&'static str, &'static str) {
    match kind {
        ShellKind::PowerShell => (
            "Write-Output forge-out-1; cmd /c exit 3\r",
            "1..100 | ForEach-Object { \"line $_\" }\r",
        ),
        ShellKind::Cmd => (
            "echo forge-out-1\r",
            "for /L %i in (1,1,100) do @echo line %i\r",
        ),
        _ => (
            "echo forge-out-1; (exit 3)\r",
            "for i in $(seq 1 100); do echo line $i; done\r",
        ),
    }
}

#[test]
fn installed_shells_mark_prompts_and_commands() {
    let mut verified = Vec::new();
    for shell in detect_shells() {
        let bash = shell.program.to_ascii_lowercase().contains("bash");
        let integrated = matches!(shell.kind, ShellKind::PowerShell | ShellKind::Cmd)
            || (matches!(shell.kind, ShellKind::GitBash | ShellKind::Posix) && bash);
        if !integrated {
            continue;
        }
        let (session, rx) = spawn(&shell);
        wait(&rx, &format!("{}: first prompt mark", shell.name), || {
            session.has_prompt_marks()
        });

        let (fail, many) = commands(shell.kind);
        session.input(fail.as_bytes().to_vec());
        // cmd marks prompts only: the block between the last two prompts.
        let reports_exit = shell.kind != ShellKind::Cmd;
        let mut last = None;
        wait(
            &rx,
            &format!("{}: output of the first command", shell.name),
            || match session.last_command_output() {
                Ok(out) if out.text.contains("forge-out-1") && !out.running => {
                    last = Some(out);
                    true
                }
                _ => false,
            },
        );
        let out = last.unwrap();
        if reports_exit {
            assert_eq!(out.exit, Some(3), "{}: {out:?}", shell.name);
            // The prompt (possibly several lines) with the typed command comes first,
            // then the output.
            let typed = out.text.find(fail.trim_end());
            let printed = out.text.rfind("forge-out-1");
            assert!(
                typed.is_some() && typed < printed,
                "{}: {out:?}",
                shell.name
            );
        }
        if shell.kind == ShellKind::PowerShell
            && let Some(command) = &out.command
        {
            assert_eq!(command, fail.trim_end());
        }

        // A failed command is marked in the margin.
        if reports_exit {
            let marks = session.snapshot(&Palette::default()).prompt_marks;
            assert!(
                marks.iter().any(|(_, m)| *m == PromptMark::Failed(3)),
                "{}: {marks:?}",
                shell.name
            );
        }

        // Enough output to push the prompts into the scrollback, then navigate.
        session.input(many.as_bytes().to_vec());
        wait(&rx, &format!("{}: 100 lines", shell.name), || {
            session
                .last_command_output()
                .is_ok_and(|o| o.text.contains("line 100") && !o.running)
        });
        assert!(
            session.jump_to_prompt(true),
            "{}: jump to older",
            shell.name
        );
        let snap = session.snapshot(&Palette::default());
        assert!(snap.display_offset > 0, "{}", shell.name);
        assert_eq!(
            snap.prompt_marks.first().map(|m| m.0),
            Some(0),
            "{}: the prompt is at the top: {:?}",
            shell.name,
            snap.prompt_marks
        );
        // Newer, until back on the live screen.
        let mut steps = 0;
        while session.jump_to_prompt(false) {
            steps += 1;
            assert!(steps < 10, "{}: no end", shell.name);
        }
        assert_eq!(session.snapshot(&Palette::default()).display_offset, 0);

        session.input(b"exit\r".to_vec());
        verified.push(shell.name.clone());
    }
    println!("command marks verified in: {verified:?}");
}

#[test]
fn program_status_query_is_answered_and_reports_kept() {
    let Some(shell) = detect_shells().into_iter().find(|s| {
        matches!(s.kind, ShellKind::GitBash | ShellKind::Posix)
            && s.program.to_ascii_lowercase().contains("bash")
    }) else {
        println!("no bash installed: skipped");
        return;
    };
    let (session, rx) = spawn(&shell);
    wait(&rx, "first prompt", || session.has_prompt_marks());
    // The reply arrives as input: read it up to the ST's backslash and show it.
    // Windows: ConPTY drops OSC sequences written to its input (measured on Windows 11
    // 26300: OSC 7501 and OSC 11 replies vanish, CSI/DCS pass), so no program behind
    // ConPTY can receive this reply. Only checked where the PTY passes bytes through.
    #[cfg(unix)]
    session.input(
        br"printf '\e]7501;?\e\\'; IFS= read -r -s -t 5 -d '\' r; echo reply:${r:1}:end"
            .iter()
            .copied()
            .chain(*b"\r")
            .collect::<Vec<u8>>(),
    );
    #[cfg(unix)]
    wait(&rx, "query reply", || {
        screen(&session).contains("reply:]7501;?")
    });

    // `done` survives the next prompt; the user's acknowledgement clears it.
    session.input(b"printf '\\e]7501;state=done:app=forge-test\\e\\\\'\r".to_vec());
    wait(&rx, "status report", || session.program_status().is_some());
    let status = session.program_status().unwrap();
    assert_eq!(status.state, ProgramState::Done);
    assert_eq!(status.app.as_deref(), Some("forge-test"));
    assert!(session.acknowledge_status());
    assert!(session.program_status().is_none());

    // `working` is dropped by the next prompt.
    session.input(b"printf '\\e]7501;state=working\\e\\\\'; sleep 1\r".to_vec());
    wait(&rx, "working", || {
        session
            .program_status()
            .is_some_and(|s| s.state == ProgramState::Working)
    });
    wait(&rx, "cleared by the prompt", || {
        session.program_status().is_none()
    });
    session.input(b"exit\r".to_vec());
}
