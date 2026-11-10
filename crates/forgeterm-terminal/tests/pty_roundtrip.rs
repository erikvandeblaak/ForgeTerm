//! End-to-end: real shell in a real PTY/ConPTY, input in, emulated output out.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use forgeterm_terminal::{GridSize, Palette, Session, SessionConfig, SessionEvent, ShellSpec};

#[cfg(windows)]
fn test_shell() -> (ShellSpec, &'static str) {
    // `set /a` prints only the result, so the marker cannot match the echoed command.
    (
        ShellSpec::new("cmd", "cmd.exe", vec![]),
        "set /a 1234*1000\r",
    )
}

#[cfg(unix)]
fn test_shell() -> (ShellSpec, &'static str) {
    (
        ShellSpec::new("sh", "/bin/sh", vec![]),
        "echo $((1234*1000))\r",
    )
}

fn wait_for(session: &Session, rx: &mpsc::Receiver<SessionEvent>, needle: &str) -> String {
    let palette = Palette::default();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let text = session.snapshot(&palette).text();
        if text.contains(needle) {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {needle:?}; screen:\n{text}"
        );
        let _ = rx.recv_timeout(Duration::from_millis(200));
    }
}

#[test]
fn shell_executes_command_and_output_is_emulated() {
    let (shell, command) = test_shell();
    let (tx, rx) = mpsc::channel();
    let config = SessionConfig {
        shell: Some(shell),
        size: GridSize::new(100, 30, 8, 16),
        ..Default::default()
    };
    let mut session = Session::spawn(config, move |e| {
        let _ = tx.send(e);
    })
    .expect("spawn shell");

    session.input(command.as_bytes().to_vec());
    wait_for(&session, &rx, "1234000");

    // Resizing must keep the session alive and reflow the grid.
    session.resize(GridSize::new(60, 20, 8, 16));
    let snap = session.snapshot(&Palette::default());
    assert_eq!((snap.cols, snap.rows), (60, 20));

    session.input(b"exit\r".to_vec());
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(SessionEvent::Exited(_)) => break,
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(e) => panic!("channel closed: {e}"),
        }
        assert!(Instant::now() < deadline, "shell did not exit");
    }
}
