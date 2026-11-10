//! Git integration through the `git` command line (porcelain formats, which git
//! documents as stable for scripts).
//!
//! Safety, because ForgeTerm may look at repositories the user did not create:
//! - read commands never run repository-configured programs: `core.fsmonitor` is
//!   switched off, diffs use `--no-ext-diff --no-textconv`;
//! - `--no-optional-locks`: background status refreshes never take `index.lock`, so
//!   they cannot make the user's own git commands fail;
//! - git never prompts (`GIT_TERMINAL_PROMPT=0`, stdin closed), every call has a
//!   timeout, and on Windows no console window appears.
//!
//! Only `stage`, `unstage` and `commit` change anything, and only when the user
//! asks; a commit runs the repository's hooks exactly like `git commit` would.

mod history;
mod refs;
mod run;
mod status;

pub use history::activity;
pub use history::{Activity, HistoryEntry, daily_counts, history, parse_history, show_commit};
pub use refs::{
    BranchRef, branches, create_branch, parse_branches, switch, track, validate_branch_name,
};
pub use run::{Output, git};
pub use status::{Branch, Change, Entry, Status, parse_status};

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Timeout for read commands (status, diff, log).
pub const READ_TIMEOUT: Duration = Duration::from_secs(15);
/// A commit runs hooks (linters, tests), which can take a while.
pub const COMMIT_TIMEOUT: Duration = Duration::from_secs(120);
/// Diffs longer than this are cut off (and marked as such).
pub const MAX_DIFF_LINES: usize = 5000;

/// The repository root containing `dir`, `Ok(None)` when `dir` is not inside a
/// repository, `Err` when git itself could not be run.
pub fn discover(dir: &Path) -> Result<Option<PathBuf>, String> {
    let out = git(dir, &["rev-parse", "--show-toplevel"], READ_TIMEOUT)?;
    if !out.success() {
        // "not a git repository" (exit 128) and the like.
        return Ok(None);
    }
    let top = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if top.is_empty() {
        return Ok(None);
    }
    Ok(Some(native_path(&top)))
}

/// Git prints `C:/x/y` on Windows; use the platform's separators.
fn native_path(p: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(p.replace('/', "\\"))
    } else {
        PathBuf::from(p)
    }
}

pub fn status(root: &Path) -> Result<Status, String> {
    let out = git(
        root,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ],
        READ_TIMEOUT,
    )?;
    out.check()?;
    Ok(parse_status(&out.stdout))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    /// Working tree against the index (`git diff`).
    Unstaged,
    /// Index against HEAD (`git diff --cached`).
    Staged,
    /// A new file git does not track yet (shown as all added).
    Untracked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diff {
    pub lines: Vec<String>,
    /// More than [`MAX_DIFF_LINES`] lines; the rest is not shown.
    pub truncated: bool,
}

/// Diff of one file (`path` relative to the repository root, as in [`Status`]).
pub fn diff(root: &Path, path: &str, kind: DiffKind) -> Result<Diff, String> {
    let mut args = vec!["diff", "--no-color", "--no-ext-diff", "--no-textconv"];
    match kind {
        DiffKind::Unstaged => {}
        DiffKind::Staged => args.push("--cached"),
        // `--no-index` compares two files; exit code 1 means "they differ".
        DiffKind::Untracked => args.push("--no-index"),
    }
    args.push("--");
    if kind == DiffKind::Untracked {
        args.push("/dev/null");
    }
    args.push(path);
    let out = git(root, &args, READ_TIMEOUT)?;
    let ok = out.success() || (kind == DiffKind::Untracked && out.code == Some(1));
    if !ok {
        return Err(out.error_text());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let truncated = lines.len() > MAX_DIFF_LINES;
    lines.truncate(MAX_DIFF_LINES);
    Ok(Diff { lines, truncated })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub author: String,
    /// Relative date as git formats it ("2 hours ago").
    pub when: String,
    pub subject: String,
}

/// The last `count` commits of HEAD; empty for a repository without commits.
pub fn log(root: &Path, count: usize) -> Result<Vec<Commit>, String> {
    let n = format!("-n{count}");
    let out = git(
        root,
        &[
            "log",
            &n,
            "--no-color",
            "--no-show-signature",
            "--format=%h%x1f%an%x1f%ar%x1f%s%x1e",
        ],
        READ_TIMEOUT,
    )?;
    if !out.success() {
        // An unborn branch has no log; anything else is a real error.
        if out.stderr.contains("does not have any commits") {
            return Ok(Vec::new());
        }
        return Err(out.error_text());
    }
    Ok(parse_log(&String::from_utf8_lossy(&out.stdout)))
}

pub fn parse_log(text: &str) -> Vec<Commit> {
    text.split('\u{1e}')
        .filter_map(|record| {
            let record = record.trim_start_matches(['\r', '\n']);
            let mut f = record.split('\u{1f}');
            let hash = f.next()?.trim();
            if hash.is_empty() {
                return None;
            }
            Some(Commit {
                hash: hash.to_owned(),
                author: f.next().unwrap_or_default().to_owned(),
                when: f.next().unwrap_or_default().to_owned(),
                subject: f.next().unwrap_or_default().trim_end().to_owned(),
            })
        })
        .collect()
}

/// `git add -- <paths>` (also stages deletions and new files).
pub fn stage(root: &Path, paths: &[String]) -> Result<(), String> {
    let mut args = vec!["add", "--"];
    args.extend(paths.iter().map(String::as_str));
    git(root, &args, READ_TIMEOUT)?.check()
}

/// Removes paths from the index, keeping the working tree (`git restore --staged`;
/// before the first commit `git rm --cached`, since there is no HEAD to restore).
pub fn unstage(root: &Path, paths: &[String], has_commits: bool) -> Result<(), String> {
    let mut args = if has_commits {
        vec!["restore", "--staged", "--"]
    } else {
        vec!["rm", "--cached", "--quiet", "--"]
    };
    args.extend(paths.iter().map(String::as_str));
    git(root, &args, READ_TIMEOUT)?.check()
}

/// Commits what is staged with `message`; returns git's summary line.
pub fn commit(root: &Path, message: &str) -> Result<String, String> {
    if message.trim().is_empty() {
        return Err("the commit message is empty".into());
    }
    let out = git(root, &["commit", "--quiet", "-m", message], COMMIT_TIMEOUT)?;
    out.check()?;
    let head = git(
        root,
        &["log", "-n1", "--no-color", "--format=%h %s"],
        READ_TIMEOUT,
    )?;
    Ok(String::from_utf8_lossy(&head.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parses_log_records() {
        let text = "abc1234\u{1f}Erik\u{1f}2 hours ago\u{1f}Fix: a | b\u{1e}\n\
                    def5678\u{1f}Bot\u{1f}3 days ago\u{1f}Init\u{1e}\n";
        let log = parse_log(text);
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].hash, "abc1234");
        assert_eq!(log[0].subject, "Fix: a | b");
        assert_eq!(log[1].when, "3 days ago");
        assert!(parse_log("").is_empty());
    }

    /// A temporary repository driven through the real git (skipped without git).
    struct Repo(PathBuf);

    impl Repo {
        fn new(name: &str) -> Option<Repo> {
            let dir =
                std::env::temp_dir().join(format!("forgeterm-git-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("sub")).unwrap();
            let out = git(&dir, &["init", "-q", "-b", "main"], READ_TIMEOUT).ok()?;
            out.success().then_some(())?;
            for (k, v) in [
                ("user.name", "Test"),
                ("user.email", "test@example.invalid"),
            ] {
                git(&dir, &["config", k, v], READ_TIMEOUT)
                    .unwrap()
                    .check()
                    .unwrap();
            }
            Some(Repo(dir))
        }

        fn write(&self, rel: &str, text: &str) {
            fs::write(self.0.join(rel), text).unwrap();
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn full_cycle_against_real_git() {
        let Some(repo) = Repo::new("cycle") else {
            eprintln!("git not available; skipped");
            return;
        };
        let root = &repo.0;

        // Discovery from a subfolder finds the root; outside a repo = None.
        let found = discover(&root.join("sub")).unwrap().unwrap();
        assert_eq!(
            fs::canonicalize(&found).unwrap(),
            fs::canonicalize(root).unwrap()
        );
        let outside = std::env::temp_dir().join(format!("forgeterm-norepo-{}", std::process::id()));
        fs::create_dir_all(&outside).unwrap();
        // The temp dir itself might live inside a repository on some machines.
        if discover(&std::env::temp_dir()).unwrap().is_none() {
            assert_eq!(discover(&outside).unwrap(), None);
        }
        let _ = fs::remove_dir_all(&outside);

        // Unborn branch: no commits, no log.
        let st = status(root).unwrap();
        assert_eq!(st.branch.head.as_deref(), Some("main"));
        assert!(!st.branch.has_commits());
        assert!(log(root, 5).unwrap().is_empty());

        // New file with a space and non-ASCII name: untracked, diff shows it added.
        repo.write("sub/hé llo.txt", "one\n");
        let st = status(root).unwrap();
        assert_eq!(st.entries.len(), 1);
        let e = &st.entries[0];
        assert_eq!(e.path, "sub/hé llo.txt");
        assert!(e.untracked);
        let d = diff(root, &e.path, DiffKind::Untracked).unwrap();
        assert!(d.lines.iter().any(|l| l == "+one"), "{:?}", d.lines);

        // Stage, unstage before the first commit, stage again, commit.
        stage(root, std::slice::from_ref(&e.path)).unwrap();
        let st = status(root).unwrap();
        assert_eq!(st.entries[0].staged, Some(Change::Added));
        assert_eq!(st.entries[0].path, e.path);
        unstage(root, std::slice::from_ref(&e.path), false).unwrap();
        assert!(status(root).unwrap().entries[0].untracked);
        stage(root, std::slice::from_ref(&e.path)).unwrap();
        assert!(commit(root, "   ").is_err());
        let summary = commit(root, "First commit").unwrap();
        assert!(summary.ends_with("First commit"), "{summary}");
        let st = status(root).unwrap();
        assert!(st.branch.has_commits());
        assert!(st.entries.is_empty());
        let history = log(root, 5).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].subject, "First commit");
        assert_eq!(history[0].author, "Test");

        // Modify: unstaged diff; stage it: staged diff; unstage with HEAD.
        repo.write("sub/hé llo.txt", "one\ntwo\n");
        let path = "sub/hé llo.txt".to_owned();
        let st = status(root).unwrap();
        assert_eq!(st.entries[0].unstaged, Some(Change::Modified));
        assert_eq!(st.entries[0].path, path);
        let d = diff(root, &path, DiffKind::Unstaged).unwrap();
        assert!(d.lines.iter().any(|l| l == "+two"));
        assert!(!d.truncated);
        stage(root, std::slice::from_ref(&path)).unwrap();
        let d = diff(root, &path, DiffKind::Staged).unwrap();
        assert!(d.lines.iter().any(|l| l == "+two"));
        unstage(root, std::slice::from_ref(&path), true).unwrap();
        let st = status(root).unwrap();
        assert_eq!(st.entries[0].staged, None);
        assert_eq!(st.entries[0].unstaged, Some(Change::Modified));

        // Errors are reported, not swallowed.
        assert!(stage(root, &["does-not-exist.txt".into()]).is_err());
    }

    #[test]
    fn branches_history_and_activity_against_real_git() {
        let Some(repo) = Repo::new("branches") else {
            return;
        };
        let root = &repo.0;
        let run =
            |dir: &Path, args: &[&str]| git(dir, args, READ_TIMEOUT).unwrap().check().unwrap();
        repo.write(
            "a.txt", "a
",
        );
        run(root, &["add", "-A"]);
        run(root, &["commit", "-q", "-m", "first"]);
        repo.write(
            "a.txt", "b
",
        );
        run(root, &["commit", "-q", "-am", "second"]);

        // Create (invalid names refused before git switch runs), list, switch back.
        assert!(create_branch(root, "").is_err());
        assert!(create_branch(root, "-x").is_err());
        assert!(create_branch(root, "bad..name").is_err());
        create_branch(root, "feature/x").unwrap();
        let list = branches(root).unwrap();
        let current: Vec<&str> = list
            .iter()
            .filter(|b| b.current)
            .map(|b| b.name.as_str())
            .collect();
        assert_eq!(current, ["feature/x"]);
        assert!(list.iter().any(|b| b.name == "main" && !b.remote));
        switch(root, "main").unwrap();
        assert!(
            branches(root)
                .unwrap()
                .iter()
                .any(|b| b.name == "main" && b.current)
        );
        assert!(switch(root, "does-not-exist").is_err());
        assert!(switch(root, "--orphan").is_err());

        // git refuses a switch that would overwrite local changes; we report it.
        run(root, &["switch", "-q", "feature/x"]);
        repo.write(
            "a.txt",
            "on feature
",
        );
        run(root, &["commit", "-q", "-am", "feature change"]);
        repo.write(
            "a.txt",
            "uncommitted
",
        );
        let refused = switch(root, "main").unwrap_err();
        assert!(!refused.is_empty());
        run(root, &["checkout", "-q", "--", "a.txt"]);
        switch(root, "main").unwrap();

        // A clone has remote branches; tracking one creates the local branch.
        let clone = root.with_extension("clone");
        let _ = fs::remove_dir_all(&clone);
        let src = root.to_string_lossy().into_owned();
        let dst = clone.to_string_lossy().into_owned();
        run(root, &["clone", "-q", &src, &dst]);
        let remote = branches(&clone).unwrap();
        assert!(
            remote
                .iter()
                .any(|b| b.remote && b.name == "origin/feature/x")
        );
        assert!(!remote.iter().any(|b| b.name.ends_with("/HEAD")));
        let main = remote.iter().find(|b| b.name == "main").unwrap();
        assert_eq!(main.upstream.as_deref(), Some("origin/main"));
        assert_eq!((main.ahead, main.behind), (0, 0));
        track(&clone, "origin/feature/x").unwrap();
        assert!(
            branches(&clone)
                .unwrap()
                .iter()
                .any(|b| b.name == "feature/x" && b.current)
        );
        let _ = fs::remove_dir_all(&clone);

        // History and one commit's details.
        let h = history(root, 10).unwrap();
        assert_eq!(
            h.iter().map(|e| e.subject.as_str()).collect::<Vec<_>>(),
            ["second", "first"]
        );
        assert!(h[0].refs.contains("main"));
        let shown = show_commit(root, &h[0].hash).unwrap();
        assert!(shown.lines.iter().any(|l| l == "+b"));
        assert!(shown.lines.iter().any(|l| l.contains("a.txt")));
        assert!(show_commit(root, "HEAD; rm").is_err());

        // Both commits were made just now: today's bucket holds them.
        let a = activity(root, 30).unwrap();
        assert_eq!(a.days.len(), 30);
        assert_eq!(a.total, 2);
        assert_eq!(a.days.last().unwrap().1, 2);
    }

    #[test]
    fn merge_conflict_and_rename_from_real_git() {
        let Some(repo) = Repo::new("conflict") else {
            return;
        };
        let root = &repo.0;
        let run = |args: &[&str]| {
            let out = git(root, args, READ_TIMEOUT).unwrap();
            assert!(out.code.is_some(), "{args:?}");
            out
        };
        repo.write("same file.txt", "base\n");
        repo.write("old name.txt", "keep\n");
        run(&["add", "-A"]).check().unwrap();
        run(&["commit", "-q", "-m", "base"]).check().unwrap();
        run(&["checkout", "-q", "-b", "other"]).check().unwrap();
        repo.write("same file.txt", "other\n");
        run(&["commit", "-q", "-am", "other"]).check().unwrap();
        run(&["checkout", "-q", "main"]).check().unwrap();
        repo.write("same file.txt", "main\n");
        run(&["commit", "-q", "-am", "main"]).check().unwrap();
        // Conflicts: exit code 1, that is expected.
        run(&["merge", "-q", "other"]);
        run(&["mv", "old name.txt", "new name.txt"])
            .check()
            .unwrap();

        let st = status(root).unwrap();
        let conflicts: Vec<&str> = st.conflicted().map(|e| e.path.as_str()).collect();
        assert_eq!(conflicts, ["same file.txt"]);
        let renamed = st.staged().next().unwrap();
        assert_eq!(renamed.path, "new name.txt");
        assert_eq!(renamed.orig_path.as_deref(), Some("old name.txt"));
        assert_eq!(renamed.staged, Some(Change::Renamed));
        run(&["merge", "--abort"]).check().unwrap();
    }

    #[test]
    fn repository_fsmonitor_is_not_run() {
        let Some(repo) = Repo::new("fsmonitor") else {
            return;
        };
        let root = &repo.0;
        // A hostile repository config pointing core.fsmonitor at a program that
        // leaves a marker file. Our status must not run it.
        let marker = root.join("ran.txt");
        let script = if cfg!(windows) {
            let s = root.join("hook.cmd");
            fs::write(&s, format!("@echo x> \"{}\"\r\n", marker.display())).unwrap();
            s
        } else {
            let s = root.join("hook.sh");
            fs::write(&s, format!("#!/bin/sh\necho x > '{}'\n", marker.display())).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&s, fs::Permissions::from_mode(0o755)).unwrap();
            }
            s
        };
        let value = script.to_string_lossy().replace('\\', "/");
        git(root, &["config", "core.fsmonitor", &value], READ_TIMEOUT)
            .unwrap()
            .check()
            .unwrap();
        repo.write("a.txt", "a\n");
        // Control: a plain `git status` does run it (otherwise the test proves nothing).
        let _ = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(root)
            .output();
        if !marker.exists() {
            eprintln!("this git did not run the fsmonitor hook at all; skipped");
            return;
        }
        fs::remove_file(&marker).unwrap();
        status(root).unwrap();
        assert!(!marker.exists(), "status ran the repository's fsmonitor");
    }
}
