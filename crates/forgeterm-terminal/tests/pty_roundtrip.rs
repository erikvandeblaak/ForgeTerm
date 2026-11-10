//! End-to-end: real shell in a real PTY/ConPTY, input in, emulated output out.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use forgeterm_terminal::{
    GridSize, Palette, SearchOutcome, Session, SessionConfig, SessionEvent, ShellSpec,
};

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
    assert!(session.pid().is_some_and(|p| p > 0), "shell pid is known");

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

#[cfg(windows)]
const EIGHTY_LINES: &str = "for /L %i in (1,1,80) do @echo line%i\r";
#[cfg(unix)]
const EIGHTY_LINES: &str = "for i in $(seq 1 80); do echo line$i; done\r";

#[test]
fn scrollback_scroll_and_clear() {
    let (shell, _) = test_shell();
    let (tx, rx) = mpsc::channel();
    let config = SessionConfig {
        shell: Some(shell),
        size: GridSize::new(80, 20, 8, 16),
        ..Default::default()
    };
    let session = Session::spawn(config, move |e| {
        let _ = tx.send(e);
    })
    .expect("spawn shell");
    let palette = Palette::default();

    session.input(EIGHTY_LINES.as_bytes().to_vec());
    wait_for(&session, &rx, "line80");

    let snap = session.snapshot(&palette);
    assert!(
        snap.history >= 60,
        "expected scrollback, got {}",
        snap.history
    );
    assert_eq!(snap.display_offset, 0);

    session.scroll_to_top();
    let top = session.snapshot(&palette);
    assert_eq!(top.display_offset, top.history);
    assert!(
        !top.text().contains("line80"),
        "top of history must not show the end"
    );

    session.scroll_to_bottom();
    assert_eq!(session.snapshot(&palette).display_offset, 0);

    session.clear_scrollback();
    let cleared = session.snapshot(&palette);
    assert_eq!(cleared.history, 0);
    assert!(cleared.text().contains("line80"), "visible screen is kept");
}

#[test]
fn search_finds_output_literally_and_selects_it() {
    let (shell, command) = test_shell();
    let (tx, rx) = mpsc::channel();
    let config = SessionConfig {
        shell: Some(shell),
        size: GridSize::new(100, 30, 8, 16),
        ..Default::default()
    };
    let session = Session::spawn(config, move |e| {
        let _ = tx.send(e);
    })
    .expect("spawn shell");
    session.input(command.as_bytes().to_vec());
    wait_for(&session, &rx, "1234000");

    // Found, selected (so Ctrl+C copies it); searching again wraps to the same one.
    assert_eq!(session.search("1234000", true), SearchOutcome::Found);
    assert_eq!(session.selection_text().as_deref(), Some("1234000"));
    assert_eq!(session.search("1234000", true), SearchOutcome::Found);
    assert_eq!(session.selection_text().as_deref(), Some("1234000"));
    // Regex characters are literal: the typed command contains "1234*1000".
    assert_eq!(session.search("1234*1000", true), SearchOutcome::Found);
    assert_eq!(session.selection_text().as_deref(), Some("1234*1000"));
    // Smart case: capitals make it case-sensitive.
    let typed = command.trim_end().split(' ').next().unwrap();
    assert_eq!(
        session.search(&typed.to_uppercase(), false),
        SearchOutcome::NotFound
    );
    assert_eq!(session.search(typed, false), SearchOutcome::Found);
    assert_eq!(
        session.search("zz-not-on-screen-zz", true),
        SearchOutcome::NotFound
    );
    assert_eq!(session.search("", true), SearchOutcome::NotFound);
}
