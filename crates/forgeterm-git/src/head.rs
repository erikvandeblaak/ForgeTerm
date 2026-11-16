//! The checked-out branch of a repository, read from `HEAD` without starting git.
//!
//! Cheap enough to poll for every agent on the dashboard, and it runs nothing from
//! the repository (no filters, hooks or config), so it is safe in repositories the
//! user has not trusted.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    /// Branch name, or the short commit hash when detached.
    pub name: String,
    pub detached: bool,
    /// `.git` is a file: a linked worktree (or a submodule).
    pub linked_worktree: bool,
}

/// Reads `<root>/.git/HEAD`, following a `.git` file (`gitdir: …`) of a linked
/// worktree. `None` when `root` is not a repository root or `HEAD` is unreadable.
pub fn head(root: &Path) -> Option<Head> {
    let dot_git = root.join(".git");
    let meta = fs::symlink_metadata(&dot_git).ok()?;
    let (git_dir, linked) = if meta.is_dir() {
        (dot_git, false)
    } else if meta.is_file() {
        let text = read_small(&dot_git)?;
        (gitdir_from_file(&text, root)?, true)
    } else {
        return None;
    };
    let text = read_small(&git_dir.join("HEAD"))?;
    let (name, detached) = parse_head(&text)?;
    Some(Head {
        name,
        detached,
        linked_worktree: linked,
    })
}

/// Branch name from a `HEAD` file: `ref: refs/heads/x` → (`x`, false); a hash →
/// (first 8 characters, true).
pub fn parse_head(text: &str) -> Option<(String, bool)> {
    let line = text.lines().next()?.trim();
    if let Some(r) = line.strip_prefix("ref:") {
        let r = r.trim();
        let name = r.strip_prefix("refs/heads/").unwrap_or(r);
        return (!name.is_empty() && !name.chars().any(char::is_control))
            .then(|| (name.to_owned(), false));
    }
    (line.len() >= 7 && line.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| (line[..8.min(line.len())].to_owned(), true))
}

/// `gitdir: <path>` from a `.git` file; relative paths are relative to the
/// worktree. Network paths are refused (reading them would connect to a server).
fn gitdir_from_file(text: &str, root: &Path) -> Option<PathBuf> {
    let raw = text.lines().next()?.strip_prefix("gitdir:")?.trim();
    if raw.is_empty() || raw.starts_with("\\\\") || raw.starts_with("//") {
        return None;
    }
    let path = Path::new(raw);
    Some(if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    })
}

/// At most 4 KB of a regular file (HEAD and `.git` files are one line).
fn read_small(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut buf = String::new();
    file.take(4096).read_to_string(&mut buf).ok()?;
    Some(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_branches_and_detached_heads() {
        assert_eq!(
            parse_head("ref: refs/heads/main\n"),
            Some(("main".into(), false))
        );
        assert_eq!(
            parse_head("ref: refs/heads/feature/x"),
            Some(("feature/x".into(), false))
        );
        assert_eq!(
            parse_head("4f2a9c1e0b7d6a5c4b3a29180f7e6d5c4b3a2918\n"),
            Some(("4f2a9c1e".into(), true))
        );
        assert_eq!(parse_head(""), None);
        assert_eq!(parse_head("ref: refs/heads/\n"), None);
        assert_eq!(parse_head("ref: refs/heads/a\u{1b}[31m"), None);
        assert_eq!(parse_head("not a hash"), None);
    }

    #[test]
    fn gitdir_files_refuse_network_paths() {
        let root = Path::new("/work/wt");
        assert_eq!(
            gitdir_from_file("gitdir: ../main/.git/worktrees/wt\n", root),
            Some(root.join("../main/.git/worktrees/wt"))
        );
        assert_eq!(gitdir_from_file("gitdir: \\\\host\\share\\x", root), None);
        assert_eq!(gitdir_from_file("gitdir: //host/share/x", root), None);
        assert_eq!(gitdir_from_file("something else", root), None);
    }

    #[test]
    fn reads_a_real_repository_and_worktree() {
        let dir = std::env::temp_dir().join(format!("forgeterm-head-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let main = dir.join("main");
        fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
        fs::write(main.join(".git/HEAD"), "ref: refs/heads/master\n").unwrap();
        fs::write(
            main.join(".git/worktrees/wt/HEAD"),
            "ref: refs/heads/agent/claude\n",
        )
        .unwrap();
        let wt = dir.join("wt");
        fs::create_dir_all(&wt).unwrap();
        fs::write(
            wt.join(".git"),
            format!("gitdir: {}\n", main.join(".git/worktrees/wt").display()),
        )
        .unwrap();

        let h = head(&main).unwrap();
        assert_eq!(
            (h.name.as_str(), h.detached, h.linked_worktree),
            ("master", false, false)
        );
        let h = head(&wt).unwrap();
        assert_eq!(
            (h.name.as_str(), h.detached, h.linked_worktree),
            ("agent/claude", false, true)
        );
        assert_eq!(head(&dir), None);
        let _ = fs::remove_dir_all(&dir);
    }
}
