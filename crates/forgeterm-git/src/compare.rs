//! Comparing two worktrees: what each changed since their common commit, and a
//! side-by-side diff of one file between them. Facts only (paths and line counts);
//! nothing here judges which side is better.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use crate::run::git;
use crate::{Diff, MAX_DIFF_LINES, READ_TIMEOUT};

/// Untracked files counted per worktree; more are listed without line counts.
pub const MAX_UNTRACKED_COUNTED: usize = 100;
/// Untracked files larger than this are listed without a line count.
/// (Compare reloads every 10 s: this bounds the reading to a few MB.)
const MAX_COUNT_BYTES: u64 = 256 * 1024;

/// One file that differs in a worktree from the common commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    /// Relative to the worktree, with `/`.
    pub path: String,
    /// `None`: binary, or not counted (see [`ChangedFile::untracked`]).
    pub added: Option<u32>,
    pub deleted: Option<u32>,
    /// A new file git does not track (yet).
    pub untracked: bool,
}

/// The common ancestor of two commits (`git merge-base`), as a full hash.
pub fn merge_base(root: &Path, a: &str, b: &str) -> Result<String, String> {
    if !is_hash(a) || !is_hash(b) {
        return Err("not a commit hash".into());
    }
    let out = git(root, &["merge-base", "--", a, b], READ_TIMEOUT)?;
    match out.code {
        Some(0) => {}
        // Exit 1 without a message: the histories share no commit.
        Some(1) if out.stderr.trim().is_empty() => {
            return Err("the two branches share no commit".into());
        }
        _ => return Err(out.error_text()),
    }
    let hash = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if is_hash(&hash) {
        Ok(hash)
    } else {
        Err("git gave no merge base".into())
    }
}

fn is_hash(s: &str) -> bool {
    (4..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Everything in `worktree` that differs from commit `base`: committed and
/// uncommitted changes to tracked files (`git diff --numstat <base>`, no renames,
/// so paths line up between worktrees), plus untracked files that are not ignored,
/// counted as all added. Like `git diff`, this applies the worktree's clean
/// filters: callers only read a trusted worktree.
pub fn changes_since(worktree: &Path, base: &str) -> Result<Vec<ChangedFile>, String> {
    if !is_hash(base) {
        return Err("not a commit hash".into());
    }
    let out = git(
        worktree,
        &[
            "diff",
            "--numstat",
            "-z",
            "--no-renames",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--ignore-submodules=all",
            base,
            "--",
        ],
        READ_TIMEOUT,
    )?;
    out.check()?;
    let mut files: Vec<ChangedFile> =
        crate::worktree::parse_numstat(&String::from_utf8_lossy(&out.stdout))
            .into_iter()
            .map(|s| ChangedFile {
                path: s.path,
                added: s.added,
                deleted: s.deleted,
                untracked: false,
            })
            .collect();

    let out = git(
        worktree,
        &["ls-files", "--others", "--exclude-standard", "-z", "--"],
        READ_TIMEOUT,
    )?;
    out.check()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for (i, path) in text.split('\0').filter(|p| !p.is_empty()).enumerate() {
        let lines = if i < MAX_UNTRACKED_COUNTED {
            safe_join(worktree, path).and_then(|p| count_lines(&p))
        } else {
            None
        };
        files.push(ChangedFile {
            path: path.to_owned(),
            added: lines,
            deleted: lines.map(|_| 0),
            untracked: true,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// `root` joined with a relative path from git: `None` for an absolute path or
/// one that climbs out (`..`), or when a link on the way leads off this computer.
pub fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    let plain = rel
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if !plain || rel.as_os_str().is_empty() {
        return None;
    }
    let path = root.join(rel);
    forgeterm_fs::resolves_locally(&path).then_some(path)
}

/// Lines in a small regular text file (the last line counts without a newline);
/// `None` for links, devices, large or binary files.
fn count_lines(path: &Path) -> Option<u32> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_COUNT_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_COUNT_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.iter().take(8000).any(|&b| b == 0) {
        return None;
    }
    let newlines = bytes.iter().filter(|&&b| b == b'\n').count();
    let last = usize::from(bytes.last().is_some_and(|&b| b != b'\n'));
    u32::try_from(newlines + last).ok()
}

/// Files larger than this are not diffed side by side (git would read them whole,
/// and Compare diffs the selected file again on every reload).
pub const MAX_SIDE_BY_SIDE_BYTES: u64 = 2 * 1024 * 1024;

/// Unified diff of two files (`git diff --no-index`), run in `dir`. `None` stands
/// for a file that does not exist on that side. Equal files give no lines.
pub fn diff_files(dir: &Path, a: Option<&Path>, b: Option<&Path>) -> Result<Diff, String> {
    for p in [a, b].into_iter().flatten() {
        if std::fs::symlink_metadata(p).is_ok_and(|m| m.len() > MAX_SIDE_BY_SIDE_BYTES) {
            return Err(format!(
                "larger than {} MB: too large to show side by side",
                MAX_SIDE_BY_SIDE_BYTES / (1024 * 1024)
            ));
        }
    }
    let side = |p: Option<&Path>| {
        p.map_or_else(
            || "/dev/null".to_owned(),
            |p| p.to_string_lossy().into_owned(),
        )
    };
    let (a, b) = (side(a), side(b));
    let out = git(
        dir,
        &[
            "diff",
            "--no-index",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--",
            &a,
            &b,
        ],
        READ_TIMEOUT,
    )?;
    // Exit code 1 means "they differ".
    if !(out.success() || out.code == Some(1)) {
        return Err(out.error_text());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let truncated = lines.len() > MAX_DIFF_LINES;
    lines.truncate(MAX_DIFF_LINES);
    Ok(Diff { lines, truncated })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// A hunk header (`@@ … @@`): the text is on the left.
    Hunk,
    Same,
    /// A removed line paired with an added one.
    Changed,
    /// Only on the left (removed).
    Removed,
    /// Only on the right (added).
    Added,
}

/// One row of a side-by-side diff: line number and text per side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub kind: RowKind,
    pub left: Option<(u32, String)>,
    pub right: Option<(u32, String)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SideBySide {
    pub rows: Vec<Row>,
    /// git reported binary files: there are no lines to show.
    pub binary: bool,
}

/// Turns a unified diff into side-by-side rows: context lines on both sides, a run
/// of removed lines paired line by line with the added lines that follow it, the
/// rest alone on its side. Headers before the first hunk are dropped.
pub fn side_by_side(lines: &[String]) -> SideBySide {
    let mut out = SideBySide::default();
    let (mut left_no, mut right_no) = (0u32, 0u32);
    let mut removed: Vec<(u32, String)> = Vec::new();
    let mut added: Vec<(u32, String)> = Vec::new();
    let mut in_hunk = false;

    let flush =
        |rows: &mut Vec<Row>, removed: &mut Vec<(u32, String)>, added: &mut Vec<(u32, String)>| {
            let n = removed.len().max(added.len());
            let mut r = removed.drain(..);
            let mut a = added.drain(..);
            for _ in 0..n {
                let (left, right) = (r.next(), a.next());
                let kind = match (&left, &right) {
                    (Some(_), Some(_)) => RowKind::Changed,
                    (Some(_), None) => RowKind::Removed,
                    _ => RowKind::Added,
                };
                rows.push(Row { kind, left, right });
            }
        };

    for line in lines {
        if line.starts_with("Binary files ") && line.ends_with(" differ") {
            out.binary = true;
            continue;
        }
        if let Some((l, r)) = parse_hunk(line) {
            flush(&mut out.rows, &mut removed, &mut added);
            in_hunk = true;
            (left_no, right_no) = (l, r);
            out.rows.push(Row {
                kind: RowKind::Hunk,
                left: Some((0, line.clone())),
                right: None,
            });
            continue;
        }
        if !in_hunk {
            continue;
        }
        if let Some(text) = line.strip_prefix('-') {
            removed.push((left_no, text.to_owned()));
            left_no += 1;
        } else if let Some(text) = line.strip_prefix('+') {
            added.push((right_no, text.to_owned()));
            right_no += 1;
        } else if line.starts_with('\\') {
            // "\ No newline at end of file": about the line before, not a line.
        } else {
            flush(&mut out.rows, &mut removed, &mut added);
            let text = line.strip_prefix(' ').unwrap_or(line).to_owned();
            out.rows.push(Row {
                kind: RowKind::Same,
                left: Some((left_no, text.clone())),
                right: Some((right_no, text)),
            });
            left_no += 1;
            right_no += 1;
        }
    }
    flush(&mut out.rows, &mut removed, &mut added);
    out
}

/// `@@ -12,7 +12,8 @@ …` → first line numbers (12, 12). A count of 0 (`-0,0`) has
/// start 0; the next line on that side would be line 1.
fn parse_hunk(line: &str) -> Option<(u32, u32)> {
    let rest = line.strip_prefix("@@ -")?;
    let (ranges, _) = rest.split_once(" @@")?;
    let (left, right) = ranges.split_once(" +")?;
    let start = |r: &str| -> Option<u32> {
        let n: u32 = r.split(',').next()?.parse().ok()?;
        Some(n.max(1))
    };
    Some((start(left)?, start(right)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn pairs_removed_and_added_runs() {
        let diff = l(&[
            "diff --git a/x b/x",
            "--- a/x",
            "+++ b/x",
            "@@ -1,4 +1,5 @@",
            " keep",
            "-old 1",
            "-old 2",
            "+new 1",
            " middle",
            "+extra",
            "+more",
            "\\ No newline at end of file",
        ]);
        let sbs = side_by_side(&diff);
        assert!(!sbs.binary);
        let kinds: Vec<RowKind> = sbs.rows.iter().map(|r| r.kind).collect();
        use RowKind::*;
        assert_eq!(kinds, [Hunk, Same, Changed, Removed, Same, Added, Added]);
        assert_eq!(sbs.rows[1].left, Some((1, "keep".into())));
        assert_eq!(sbs.rows[2].left, Some((2, "old 1".into())));
        assert_eq!(sbs.rows[2].right, Some((2, "new 1".into())));
        assert_eq!(sbs.rows[3].left, Some((3, "old 2".into())));
        assert_eq!(sbs.rows[3].right, None);
        assert_eq!(sbs.rows[4].left, Some((4, "middle".into())));
        assert_eq!(sbs.rows[4].right, Some((3, "middle".into())));
        assert_eq!(sbs.rows[6].right, Some((5, "more".into())));
    }

    #[test]
    fn new_file_hunks_and_binaries() {
        let sbs = side_by_side(&l(&["@@ -0,0 +1,2 @@", "+a", "+b"]));
        assert_eq!(sbs.rows.len(), 3);
        assert_eq!(sbs.rows[2].right, Some((2, "b".into())));
        assert_eq!(sbs.rows[2].left, None);
        let bin = side_by_side(&l(&["Binary files /dev/null and b/logo.png differ"]));
        assert!(bin.binary && bin.rows.is_empty());
        // A second hunk restarts the numbering.
        let two = side_by_side(&l(&["@@ -1 +1 @@", "-x", "+y", "@@ -40,1 +41,1 @@", " z"]));
        assert_eq!(two.rows[3].left, Some((40, "z".into())));
        assert_eq!(two.rows[3].right, Some((41, "z".into())));
    }

    #[test]
    fn hashes_and_paths_are_checked() {
        assert!(is_hash("abc1234") && !is_hash("--all") && !is_hash("HEAD") && !is_hash("ab"));
        let root = std::env::temp_dir();
        assert!(safe_join(&root, "src/main.rs").is_some());
        assert!(safe_join(&root, "../outside").is_none());
        assert!(safe_join(&root, "a/../../b").is_none());
        assert!(safe_join(&root, "").is_none());
        #[cfg(windows)]
        assert!(safe_join(&root, r"C:\Windows\win.ini").is_none());
        #[cfg(not(windows))]
        assert!(safe_join(&root, "/etc/passwd").is_none());
    }

    #[test]
    fn large_files_are_not_diffed() {
        let dir = std::env::temp_dir().join(format!("forgeterm-big-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let big = dir.join("big.txt");
        let f = std::fs::File::create(&big).unwrap();
        f.set_len(MAX_SIDE_BY_SIDE_BYTES + 1).unwrap();
        let e = diff_files(&dir, None, Some(&big)).unwrap_err();
        assert!(e.contains("too large"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
