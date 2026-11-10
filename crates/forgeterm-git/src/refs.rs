//! Branches: list (`for-each-ref`), switch, create.

use std::path::Path;

use crate::{READ_TIMEOUT, git};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchRef {
    /// `main` or `origin/main`.
    pub name: String,
    pub remote: bool,
    /// The checked-out branch.
    pub current: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// The upstream branch no longer exists.
    pub upstream_gone: bool,
    /// Unix time of the last commit.
    pub time: i64,
    pub subject: String,
}

const FORMAT: &str = "--format=%(refname)%00%(refname:short)%00%(HEAD)%00%(upstream:short)%00%(upstream:track,nobracket)%00%(committerdate:unix)%00%(subject)";

/// Local branches (most recently committed first), then remote ones.
pub fn branches(root: &Path) -> Result<Vec<BranchRef>, String> {
    let out = git(
        root,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            FORMAT,
            "refs/heads",
            "refs/remotes",
        ],
        READ_TIMEOUT,
    )?;
    out.check()?;
    Ok(parse_branches(&String::from_utf8_lossy(&out.stdout)))
}

pub fn parse_branches(text: &str) -> Vec<BranchRef> {
    let mut list: Vec<BranchRef> = text
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\0').collect();
            if f.len() < 7 {
                return None;
            }
            let full = f[0];
            let remote = full.starts_with("refs/remotes/");
            // `origin/HEAD` is a pointer, not a branch.
            if remote && full.ends_with("/HEAD") {
                return None;
            }
            if !remote && !full.starts_with("refs/heads/") {
                return None;
            }
            let (ahead, behind, gone) = parse_track(f[4]);
            Some(BranchRef {
                name: f[1].to_owned(),
                remote,
                current: f[2] == "*",
                upstream: (!f[3].is_empty()).then(|| f[3].to_owned()),
                ahead,
                behind,
                upstream_gone: gone,
                time: f[5].trim().parse().unwrap_or(0),
                subject: f[6].to_owned(),
            })
        })
        .collect();
    // Stable: keeps the committer-date order within local and within remote.
    list.sort_by_key(|b| b.remote);
    list
}

/// `ahead 2, behind 1`, `ahead 2`, `gone`, or empty.
fn parse_track(s: &str) -> (u32, u32, bool) {
    if s.trim() == "gone" {
        return (0, 0, true);
    }
    let mut ahead = 0;
    let mut behind = 0;
    for part in s.split(',') {
        let mut words = part.split_whitespace();
        match (words.next(), words.next().and_then(|n| n.parse().ok())) {
            (Some("ahead"), Some(n)) => ahead = n,
            (Some("behind"), Some(n)) => behind = n,
            _ => {}
        }
    }
    (ahead, behind, false)
}

/// A name git accepts as a new branch (`git check-ref-format --branch`), also
/// refused when it looks like an option.
pub fn validate_branch_name(root: &Path, name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("the branch name is empty".into());
    }
    if name.starts_with('-') {
        return Err("a branch name cannot start with '-'".into());
    }
    let out = git(root, &["check-ref-format", "--branch", name], READ_TIMEOUT)?;
    if out.success() {
        Ok(())
    } else {
        Err(format!("'{name}' is not a valid branch name"))
    }
}

/// Switches to a local branch (`git switch`). git refuses when local changes would
/// be overwritten; that refusal is returned as the error.
pub fn switch(root: &Path, branch: &str) -> Result<(), String> {
    if branch.starts_with('-') {
        return Err("invalid branch name".into());
    }
    git(root, &["switch", "--no-guess", branch], READ_TIMEOUT)?.check()
}

/// Creates a local branch tracking `remote_branch` (`origin/x` → `x`) and switches
/// to it (`git switch --track`).
pub fn track(root: &Path, remote_branch: &str) -> Result<(), String> {
    if remote_branch.starts_with('-') {
        return Err("invalid branch name".into());
    }
    git(root, &["switch", "--track", remote_branch], READ_TIMEOUT)?.check()
}

/// Creates `name` at HEAD and switches to it (`git switch -c`); local changes
/// come along.
pub fn create_branch(root: &Path, name: &str) -> Result<(), String> {
    let name = name.trim();
    validate_branch_name(root, name)?;
    git(root, &["switch", "-c", name], READ_TIMEOUT)?.check()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_local_and_remote_branches() {
        // `\x00` (not `\0`) before digits: `\01` reads like an octal escape.
        let text = "refs/heads/main\0main\0*\0origin/main\0ahead 2, behind 1\x001700000000\0Fix it\n\
                    refs/remotes/origin/HEAD\0origin\0 \0\0\x001700000000\0x\n\
                    refs/remotes/origin/main\0origin/main\0 \0\0\x001699999000\0Older\n\
                    refs/heads/old\0old\0 \0origin/old\0gone\x001600000000\0Ancient\n\
                    refs/heads/topic\0topic\0 \0\0\x001650000000\0Topic work\n";
        let b = parse_branches(text);
        let names: Vec<&str> = b.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["main", "old", "topic", "origin/main"]);
        assert!(b[0].current);
        assert_eq!((b[0].ahead, b[0].behind), (2, 1));
        assert_eq!(b[0].upstream.as_deref(), Some("origin/main"));
        assert!(b[1].upstream_gone);
        assert_eq!(b[2].upstream, None);
        assert!(b[3].remote);
        assert_eq!(b[3].time, 1_699_999_000);
        assert_eq!(b[0].subject, "Fix it");
    }

    #[test]
    fn parses_track_variants() {
        assert_eq!(parse_track("ahead 3"), (3, 0, false));
        assert_eq!(parse_track("behind 4"), (0, 4, false));
        assert_eq!(parse_track(""), (0, 0, false));
        assert_eq!(parse_track("gone"), (0, 0, true));
    }
}
