//! `git status --porcelain=v2 --branch -z` parsing.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Branch {
    /// Branch name; `None` when HEAD is detached.
    pub head: Option<String>,
    /// Commit HEAD points at; `None` before the first commit.
    pub oid: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
}

impl Branch {
    pub fn has_commits(&self) -> bool {
        self.oid.is_some()
    }

    /// `main`, or `detached at 1a2b3c4`.
    pub fn label(&self) -> String {
        match (&self.head, &self.oid) {
            (Some(name), _) => name.clone(),
            (None, Some(oid)) => format!("detached at {}", &oid[..oid.len().min(7)]),
            (None, None) => "detached".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
}

impl Change {
    fn from_code(c: char) -> Option<Change> {
        Some(match c {
            'M' => Change::Modified,
            'A' => Change::Added,
            'D' => Change::Deleted,
            'R' => Change::Renamed,
            'C' => Change::Copied,
            'T' => Change::TypeChanged,
            _ => return None,
        })
    }

    /// One-letter code as git shows it.
    pub fn letter(self) -> char {
        match self {
            Change::Modified => 'M',
            Change::Added => 'A',
            Change::Deleted => 'D',
            Change::Renamed => 'R',
            Change::Copied => 'C',
            Change::TypeChanged => 'T',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Relative to the repository root, `/`-separated (as git prints it).
    pub path: String,
    /// Previous path of a rename or copy.
    pub orig_path: Option<String>,
    /// Change in the index (what the next commit contains).
    pub staged: Option<Change>,
    /// Change in the working tree that is not staged.
    pub unstaged: Option<Change>,
    pub untracked: bool,
    /// Unmerged (merge conflict).
    pub conflicted: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub branch: Branch,
    pub entries: Vec<Entry>,
}

impl Status {
    pub fn staged(&self) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(|e| e.staged.is_some() && !e.conflicted)
    }

    pub fn unstaged(&self) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(|e| e.unstaged.is_some() && !e.conflicted)
    }

    pub fn untracked(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| e.untracked)
    }

    pub fn conflicted(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| e.conflicted)
    }

    /// Number of changed paths (each path once).
    pub fn change_count(&self) -> usize {
        self.entries.len()
    }
}

/// Parses NUL-separated porcelain v2 output. Unknown records are skipped, so a
/// newer git with extra headers does not break anything.
pub fn parse_status(raw: &[u8]) -> Status {
    let text = String::from_utf8_lossy(raw);
    let mut fields = text.split('\0').filter(|f| !f.is_empty());
    let mut status = Status::default();
    while let Some(record) = fields.next() {
        if let Some(header) = record.strip_prefix("# ") {
            parse_header(header, &mut status.branch);
            continue;
        }
        let mut parts = record.splitn(2, ' ');
        let kind = parts.next().unwrap_or_default();
        let rest = parts.next().unwrap_or_default();
        match kind {
            // 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>
            "1" => {
                if let Some((xy, path)) = split_fields(rest, 6) {
                    status.entries.push(tracked(xy, path, None));
                }
            }
            // 2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <Xscore> <path> NUL <origPath>
            "2" => {
                if let Some((xy, path)) = split_fields(rest, 7) {
                    let orig = fields.next().map(str::to_owned);
                    status.entries.push(tracked(xy, path, orig));
                }
            }
            // u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>
            "u" => {
                if let Some((_, path)) = split_fields(rest, 8) {
                    status.entries.push(Entry {
                        path: path.to_owned(),
                        orig_path: None,
                        staged: None,
                        unstaged: None,
                        untracked: false,
                        conflicted: true,
                    });
                }
            }
            "?" => status.entries.push(Entry {
                path: rest.to_owned(),
                orig_path: None,
                staged: None,
                unstaged: None,
                untracked: true,
                conflicted: false,
            }),
            _ => {}
        }
    }
    status
}

fn parse_header(header: &str, branch: &mut Branch) {
    let (key, value) = header.split_once(' ').unwrap_or((header, ""));
    match key {
        "branch.oid" => branch.oid = (value != "(initial)").then(|| value.to_owned()),
        "branch.head" => branch.head = (value != "(detached)").then(|| value.to_owned()),
        "branch.upstream" => branch.upstream = Some(value.to_owned()),
        "branch.ab" => {
            for part in value.split(' ') {
                if let Some(n) = part.strip_prefix('+') {
                    branch.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    branch.behind = n.parse().unwrap_or(0);
                }
            }
        }
        _ => {}
    }
}

/// Returns the first field (XY) and the path after `skip` more space-separated
/// fields; the path itself may contain spaces.
fn split_fields(rest: &str, skip: usize) -> Option<(&str, &str)> {
    let mut parts = rest.splitn(skip + 2, ' ');
    let xy = parts.next()?;
    for _ in 0..skip {
        parts.next()?;
    }
    let path = parts.next()?;
    (xy.len() == 2 && !path.is_empty()).then_some((xy, path))
}

fn tracked(xy: &str, path: &str, orig_path: Option<String>) -> Entry {
    let mut chars = xy.chars();
    let x = chars.next().unwrap_or('.');
    let y = chars.next().unwrap_or('.');
    Entry {
        path: path.to_owned(),
        orig_path,
        staged: Change::from_code(x),
        unstaged: Change::from_code(y),
        untracked: false,
        conflicted: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from git 2.56 (Windows), then extended by hand.
    const SAMPLE: &str = "# branch.oid 4168c5cf24c4220688d2cc385d0ffdb60e5d9a00\0\
        # branch.head main\0\
        # branch.upstream origin/main\0\
        # branch.ab +2 -1\0\
        2 RM N... 100644 100644 100644 7898192 7898192 R100 moved file.txt\0a.txt\0\
        1 .M N... 100644 100644 100644 aaa bbb src/lib.rs\0\
        1 A. N... 000000 100644 100644 000 ccc new.rs\0\
        1 .D N... 100644 100644 000000 ddd ddd gone.rs\0\
        u UU N... 100644 100644 100644 100644 e1 e2 e3 conflict here.rs\0\
        ? b c.txt\0\
        ? sub/s.txt\0\
        # branch.future-header whatever\0\
        x unknown record\0";

    #[test]
    fn parses_branch_headers() {
        let s = parse_status(SAMPLE.as_bytes());
        let b = &s.branch;
        assert_eq!(b.head.as_deref(), Some("main"));
        assert_eq!(b.upstream.as_deref(), Some("origin/main"));
        assert_eq!((b.ahead, b.behind), (2, 1));
        assert!(b.has_commits());
        assert_eq!(b.label(), "main");
    }

    #[test]
    fn parses_entries_with_spaces_renames_and_conflicts() {
        let s = parse_status(SAMPLE.as_bytes());
        assert_eq!(s.change_count(), 7);
        let renamed = &s.entries[0];
        assert_eq!(renamed.path, "moved file.txt");
        assert_eq!(renamed.orig_path.as_deref(), Some("a.txt"));
        assert_eq!(renamed.staged, Some(Change::Renamed));
        assert_eq!(renamed.unstaged, Some(Change::Modified));

        let staged: Vec<&str> = s.staged().map(|e| e.path.as_str()).collect();
        assert_eq!(staged, ["moved file.txt", "new.rs"]);
        let unstaged: Vec<&str> = s.unstaged().map(|e| e.path.as_str()).collect();
        assert_eq!(unstaged, ["moved file.txt", "src/lib.rs", "gone.rs"]);
        let untracked: Vec<&str> = s.untracked().map(|e| e.path.as_str()).collect();
        assert_eq!(untracked, ["b c.txt", "sub/s.txt"]);
        let conflicts: Vec<&str> = s.conflicted().map(|e| e.path.as_str()).collect();
        assert_eq!(conflicts, ["conflict here.rs"]);
        assert_eq!(s.entries[3].unstaged.map(Change::letter), Some('D'));
    }

    #[test]
    fn initial_and_detached() {
        let s = parse_status(b"# branch.oid (initial)\0# branch.head main\0");
        assert!(!s.branch.has_commits());
        assert_eq!(s.branch.upstream, None);
        let s = parse_status(b"# branch.oid 1234567890abcdef\0# branch.head (detached)\0");
        assert_eq!(s.branch.head, None);
        assert_eq!(s.branch.label(), "detached at 1234567");
        assert!(parse_status(b"").entries.is_empty());
    }
}
