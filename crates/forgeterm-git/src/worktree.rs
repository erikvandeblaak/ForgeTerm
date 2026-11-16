//! Worktrees and per-file line counts, for the Git dashboard. Read-only.

use std::path::{Path, PathBuf};

use crate::{READ_TIMEOUT, git, native_path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// Commit checked out (full hash); `None` for a bare repository.
    pub head: Option<String>,
    /// Branch name without `refs/heads/`; `None` when detached or bare.
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    /// `Some(reason)` (possibly empty) when locked.
    pub locked: Option<String>,
    /// `Some(reason)` when git considers it prunable (its folder is gone).
    pub prunable: Option<String>,
}

/// Every worktree of the repository at `root`; the main one first, as git lists it.
pub fn worktrees(root: &Path) -> Result<Vec<Worktree>, String> {
    let out = git(
        root,
        &["worktree", "list", "--porcelain", "-z"],
        READ_TIMEOUT,
    )?;
    out.check()?;
    Ok(parse_worktrees(&String::from_utf8_lossy(&out.stdout)))
}

/// `git worktree list --porcelain -z`: NUL-terminated `key value` attributes, an
/// empty attribute ends a record. Unknown attributes are ignored (forward
/// compatible, as git documents).
pub fn parse_worktrees(text: &str) -> Vec<Worktree> {
    let mut list = Vec::new();
    let mut current: Option<Worktree> = None;
    for field in text.split('\0') {
        if field.is_empty() {
            list.extend(current.take());
            continue;
        }
        let (key, value) = field.split_once(' ').unwrap_or((field, ""));
        if key == "worktree" {
            list.extend(current.take());
            current = Some(Worktree {
                path: native_path(value),
                head: None,
                branch: None,
                detached: false,
                bare: false,
                locked: None,
                prunable: None,
            });
            continue;
        }
        let Some(w) = current.as_mut() else {
            continue;
        };
        match key {
            "HEAD" => w.head = Some(value.to_owned()),
            "branch" => {
                w.branch = Some(
                    value
                        .strip_prefix("refs/heads/")
                        .unwrap_or(value)
                        .to_owned(),
                );
            }
            "detached" => w.detached = true,
            "bare" => w.bare = true,
            "locked" => w.locked = Some(value.to_owned()),
            "prunable" => w.prunable = Some(value.to_owned()),
            _ => {}
        }
    }
    list.extend(current);
    list
}

/// Lines added and removed in one file; `None` for binary files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStat {
    /// Path relative to the root (the new path of a rename).
    pub path: String,
    pub added: Option<u32>,
    pub deleted: Option<u32>,
}

/// Line counts of the unstaged (`staged = false`) or staged changes.
pub fn numstat(root: &Path, staged: bool) -> Result<Vec<FileStat>, String> {
    let mut args = vec![
        "diff",
        "--numstat",
        "-z",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--ignore-submodules=all",
    ];
    if staged {
        args.push("--cached");
    }
    let out = git(root, &args, READ_TIMEOUT)?;
    out.check()?;
    Ok(parse_numstat(&String::from_utf8_lossy(&out.stdout)))
}

/// `git diff --numstat -z`: `added\tdeleted\tpath\0`, or for a rename
/// `added\tdeleted\t\0old\0new\0`. Binary files have `-` counts.
pub fn parse_numstat(text: &str) -> Vec<FileStat> {
    let mut out = Vec::new();
    let mut fields = text.split('\0');
    while let Some(record) = fields.next() {
        if record.is_empty() {
            continue;
        }
        let mut parts = record.splitn(3, '\t');
        let (Some(a), Some(d), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let path = if path.is_empty() {
            // Rename: old path, then new path, as separate fields.
            let _old = fields.next();
            match fields.next() {
                Some(new) => new.to_owned(),
                None => continue,
            }
        } else {
            path.to_owned()
        };
        out.push(FileStat {
            path,
            added: a.parse().ok(),
            deleted: d.parse().ok(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_worktree_records() {
        let text = "worktree C:/work/shop\0HEAD 1111111111111111111111111111111111111111\0branch refs/heads/main\0\0\
                    worktree C:/work/shop-wt/claude\0HEAD 2222222222222222222222222222222222222222\0branch refs/heads/agent/claude\0locked\0\0\
                    worktree C:/work/old\0HEAD 3333333333333333333333333333333333333333\0detached\0prunable gitdir file points to non-existent location\0future-key x\0\0";
        let list = parse_worktrees(text);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].branch.as_deref(), Some("main"));
        assert_eq!(list[0].path, native_path("C:/work/shop"));
        assert!(!list[0].detached && list[0].locked.is_none());
        assert_eq!(list[1].branch.as_deref(), Some("agent/claude"));
        assert_eq!(list[1].locked.as_deref(), Some(""));
        assert!(list[2].detached && list[2].branch.is_none());
        assert!(
            list[2]
                .prunable
                .as_deref()
                .unwrap()
                .contains("non-existent")
        );
        assert!(parse_worktrees("").is_empty());
        // A bare main repository, no trailing terminator.
        let bare = parse_worktrees("worktree /srv/x.git\0bare");
        assert!(bare[0].bare && bare[0].head.is_none());
    }

    #[test]
    fn parses_numstat_with_renames_and_binaries() {
        let text = "3\t1\tsrc/main.rs\0-\t-\tlogo.png\0\
                    0\t0\t\0old name.txt\0new name.txt\0\
                    12\t0\tdocs/é.md\0";
        let s = parse_numstat(text);
        assert_eq!(s.len(), 4);
        assert_eq!((s[0].added, s[0].deleted), (Some(3), Some(1)));
        assert_eq!((s[1].path.as_str(), s[1].added), ("logo.png", None));
        assert_eq!(s[2].path, "new name.txt");
        assert_eq!(s[3].path, "docs/é.md");
        assert!(parse_numstat("").is_empty());
    }
}
