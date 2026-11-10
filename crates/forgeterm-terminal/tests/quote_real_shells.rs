//! Feeds quoted paths to the real shells installed on this machine and checks that
//! each shell resolves them to exactly the intended file.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use forgeterm_terminal::{ShellKind, ShellSpec, detect_shells, quote_path};

/// Names legal on every supported OS but full of shell metacharacters.
const NASTY: &[&str] = &[
    "plain.txt",
    "with space.txt",
    "it's $HOME & (1).txt",
    "typo \u{2018}single\u{2019} quotes.txt",
    "brackets [a] {b}; c,d=e^f!g#h@i`j~k+l.txt",
    // `%VAR%` expands even inside quotes in interactive cmd; tested for other shells.
    "percent %PATH% sign.txt",
];

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("forgeterm quote test {}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs `shell` so that it parses `quoted` exactly as typed at its prompt and prints
/// the file's contents. `None` = shell not exercised by this test.
fn read_with(shell: &ShellSpec, quoted: &str) -> Option<String> {
    let mut cmd = Command::new(&shell.program);
    match shell.kind {
        ShellKind::PowerShell => {
            cmd.args(["-NoProfile", "-NonInteractive", "-Command"])
                .arg(format!("Get-Content -LiteralPath {quoted}"));
        }
        ShellKind::Cmd => {
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.raw_arg(format!("/D /C type {quoted}"));
            }
        }
        ShellKind::GitBash | ShellKind::Posix => {
            cmd.args(["--noprofile", "--norc", "-c"])
                .arg(format!("cat -- {quoted}"));
            if shell.program.ends_with("/sh") {
                return None;
            }
        }
        ShellKind::Fish => {
            cmd.args(["--no-config", "-c"])
                .arg(format!("cat -- {quoted}"));
        }
        // Starting WSL boots a Linux distribution; not done from a unit test run.
        ShellKind::Wsl => return None,
    }
    let out = cmd.output().expect("shell starts");
    assert!(
        out.status.success(),
        "{} failed for {quoted}: {}",
        shell.name,
        String::from_utf8_lossy(&out.stderr)
    );
    Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

#[test]
fn quoted_paths_reach_the_right_file_in_every_installed_shell() {
    let dir = temp_dir();
    let files: Vec<(PathBuf, String)> = NASTY
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let path = dir.join(name);
            let marker = format!("forgeterm-marker-{i}");
            fs::write(&path, &marker).unwrap();
            (path, marker)
        })
        .collect();

    let mut exercised = Vec::new();
    for shell in detect_shells() {
        let mut ran = false;
        for (path, marker) in &files {
            if shell.kind == ShellKind::Cmd && path.to_string_lossy().contains('%') {
                continue;
            }
            let quoted = quote_path(shell.kind, &path_str(path));
            if let Some(got) = read_with(&shell, &quoted) {
                assert_eq!(
                    &got, marker,
                    "{} read the wrong file via {quoted}",
                    shell.name
                );
                ran = true;
            }
        }
        if ran {
            exercised.push(shell.name.clone());
        }
    }
    let _ = fs::remove_dir_all(&dir);
    println!("quoting verified in: {exercised:?}");
}

fn path_str(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
