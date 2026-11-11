//! File references in terminal text, for Ctrl+click → code viewer:
//!
//! | Form | Example | Used by |
//! |---|---|---|
//! | `path:line[:col]` | `src/main.rs:42:5` | rustc, gcc/clang, tsc, eslint, go, grep -n |
//! | `path(line[,col])` | `Program.cs(12,5)` | MSVC, .NET |
//! | `File "path", line N` | `File "app.py", line 3` | Python tracebacks |
//! | `path` with a folder and an extension | `src/lib.rs` | listings, git output |
//!
//! A path needs a file extension or a folder separator, so `localhost:8080` or
//! `version 1.2` are not references. Terminal output is untrusted: a reference is
//! only ever *opened read-only in the viewer*, after [`resolve`] found a regular
//! local file. Network paths (UNC) are refused, so a click never makes Windows
//! contact a server.

use std::path::{Path, PathBuf};

/// Longest path accepted (characters).
const MAX_PATH_CHARS: usize = 1024;
/// Highest line/column number accepted.
const MAX_NUMBER: u32 = 10_000_000;

/// A file reference in a line: char range `[start, end)` of the path text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRef {
    pub start: usize,
    pub end: usize,
    pub path: String,
    /// 1-based.
    pub line: Option<u32>,
    /// 1-based.
    pub column: Option<u32>,
}

/// The file reference covering char index `at`, unless a web link covers it.
pub fn file_ref_at(line: &str, at: usize) -> Option<FileRef> {
    if crate::url::link_at(line, at).is_some() {
        return None;
    }
    file_refs(line)
        .into_iter()
        .find(|r| r.start <= at && at < r.end)
}

/// All file references in `line`, left to right.
pub fn file_refs(line: &str) -> Vec<FileRef> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = python_refs(&chars);
    let mut i = 0;
    while i < chars.len() {
        if is_separator(chars[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && !is_separator(chars[i]) {
            i += 1;
        }
        if let Some(r) = token_ref(&chars, start, i)
            && !out.iter().any(|o| o.start < r.end && r.start < o.end)
        {
            out.push(r);
        }
    }
    out.sort_by_key(|r| r.start);
    out
}

/// Characters that end a path token (paths with spaces are only found quoted,
/// in Python tracebacks).
fn is_separator(c: char) -> bool {
    c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '`' | '<' | '>' | '|')
}

/// `File "path", line N`
fn python_refs(chars: &[char]) -> Vec<FileRef> {
    const PREFIX: &str = "File \"";
    const MIDDLE: &str = "\", line ";
    let text: String = chars.iter().collect();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(pos) = text[from..].find(PREFIX) {
        let path_start = from + pos + PREFIX.len();
        from = path_start;
        let Some(len) = text[path_start..].find('"') else {
            break;
        };
        let path_end = path_start + len;
        let Some(rest) = text[path_end..].strip_prefix(MIDDLE) else {
            continue;
        };
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        let path = &text[path_start..path_end];
        if let (Some(line), true) = (number(&digits), plausible_path(path, true)) {
            let start = text[..path_start].chars().count();
            out.push(FileRef {
                start,
                end: start + path.chars().count(),
                path: path.to_owned(),
                line: Some(line),
                column: None,
            });
        }
    }
    out
}

/// One whitespace-separated token: `path:line[:col]`, `path(line[,col])` or a
/// bare path.
fn token_ref(chars: &[char], start: usize, end: usize) -> Option<FileRef> {
    let mut s = start;
    let mut e = end;
    // Opening brackets glued to the front, sentence punctuation at the end.
    while s < e && matches!(chars[s], '(' | '[' | '{') {
        s += 1;
    }
    while e > s && matches!(chars[e - 1], ',' | ';' | '.' | ':' | ']' | '}') {
        e -= 1;
    }
    let token: String = chars[s..e].iter().collect();
    if token.contains("://") {
        return None;
    }

    // path(line[,col])
    if let Some(inner) = token.strip_suffix(')')
        && let Some((path, nums)) = inner.rsplit_once('(')
    {
        let numbers = match nums.split_once(',') {
            Some((l, c)) => number(l).zip(number(c)).map(|(l, c)| (l, Some(c))),
            None => number(nums).map(|l| (l, None)),
        };
        if let Some((line, column)) = numbers {
            return make(s, path, Some(line), column, true);
        }
        // An agent's tool call such as Claude Code's `Update(src/main.rs)`: the
        // path inside the parentheses.
        let inside = s + path.chars().count() + 1;
        return path_with_range(inside, nums).or_else(|| make(inside, nums, None, None, false));
    }
    // An unbalanced `)` after a path: `(see src/a.rs:3)`.
    let token = if token.ends_with(')') && !token.contains('(') {
        token[..token.len() - 1]
            .trim_end_matches([',', ';', '.', ':'])
            .to_owned()
    } else {
        token
    };

    if let Some(r) = path_with_range(s, &token) {
        return Some(r);
    }
    // path:line[:col]
    if let Some((rest, last)) = token.rsplit_once(':')
        && let Some(last) = number(last)
    {
        if let Some((path, line)) = rest.rsplit_once(':')
            && let Some(line) = number(line)
            && !path.is_empty()
        {
            return make(s, path, Some(line), Some(last), true);
        }
        return make(s, rest, Some(last), None, true);
    }
    make(s, &token, None, None, false)
}

/// `path:first-last` (a line range, as agents print it): opens at `first`.
fn path_with_range(start: usize, token: &str) -> Option<FileRef> {
    let (path, range) = token.rsplit_once(':')?;
    let (first, last) = range.split_once('-')?;
    let first = number(first)?;
    number(last)?;
    make(start, path, Some(first), None, true)
}

fn make(
    start: usize,
    path: &str,
    line: Option<u32>,
    column: Option<u32>,
    has_number: bool,
) -> Option<FileRef> {
    if !plausible_path(path, has_number) {
        return None;
    }
    Some(FileRef {
        start,
        end: start + path.chars().count(),
        path: path.to_owned(),
        line,
        column,
    })
}

/// 1..=MAX_NUMBER, digits only.
fn number(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok().filter(|n| (1..=MAX_NUMBER).contains(n))
}

/// A file name with an extension (`main.rs`, `.env.local` excluded: no stem) or,
/// without a line number, also a folder separator.
fn plausible_path(path: &str, has_number: bool) -> bool {
    if path.is_empty()
        || path.chars().count() > MAX_PATH_CHARS
        || path.starts_with('-')
        || path.chars().any(char::is_control)
    {
        return false;
    }
    // A colon only as a drive letter (`C:\…`): log timestamps such as
    // `11/Nov/2026:10:30:45` are not paths.
    let colons: Vec<usize> = path.match_indices(':').map(|(i, _)| i).collect();
    let b = path.as_bytes();
    let drive = b.len() > 2 && b[0].is_ascii_alphabetic() && matches!(b[2], b'\\' | b'/');
    if !(colons.is_empty() || (colons == [1] && drive)) {
        return false;
    }
    let name = path.rsplit(['/', '\\']).next().unwrap_or_default();
    // Windows device names (`NUL.txt` is the NUL device there): never a file.
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit())
    {
        return false;
    }
    let has_folder = path.contains(['/', '\\']) && !name.is_empty();
    let has_extension = name.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty()
            && (1..=10).contains(&ext.len())
            && ext.starts_with(|c: char| c.is_ascii_alphabetic())
            && ext.chars().all(|c| c.is_ascii_alphanumeric())
            // `github.com:22`, `example.org:443`: a host and port, not a file.
            && (has_folder || !is_common_tld(ext))
    });
    if has_number {
        has_extension || has_folder
    } else {
        has_extension && has_folder
    }
}

/// Top-level domains seen as `host:port` in logs (case-insensitive). None of these
/// is a common source-file extension.
fn is_common_tld(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "com"
            | "org"
            | "net"
            | "io"
            | "dev"
            | "app"
            | "nl"
            | "de"
            | "be"
            | "uk"
            | "eu"
            | "local"
            | "internal"
            | "lan"
            | "localhost"
    )
}

/// The local file a reference points to: absolute as is, relative against each
/// of `bases` in order. `None` for network paths (UNC, `//host`), device paths
/// and anything that is not an existing regular file per `is_file`. Every
/// candidate must pass [`crate::is_local_path`] *before* `is_file` touches the
/// file system (same rule as working-directory reports).
pub fn resolve(path: &str, bases: &[PathBuf], is_file: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let local_file = |p: &Path| p.to_str().is_some_and(crate::is_local_path) && is_file(p);
    let p = Path::new(path);
    if p.is_absolute() || p.has_root() {
        // `\foo` on Windows has a root but no drive: relative to the current drive,
        // which ForgeTerm does not know. Only fully absolute, local paths.
        return local_file(p).then(|| p.to_path_buf());
    }
    bases
        .iter()
        .map(|b| b.join(p))
        .find(|candidate| local_file(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs(line: &str) -> Vec<(String, Option<u32>, Option<u32>)> {
        file_refs(line)
            .into_iter()
            .map(|r| (r.path, r.line, r.column))
            .collect()
    }

    fn one(
        path: &str,
        line: Option<u32>,
        col: Option<u32>,
    ) -> Vec<(String, Option<u32>, Option<u32>)> {
        vec![(path.to_owned(), line, col)]
    }

    #[test]
    fn compiler_forms() {
        assert_eq!(
            refs("  --> src/main.rs:42:5"),
            one("src/main.rs", Some(42), Some(5))
        );
        assert_eq!(
            refs("src/app.ts:7:13: error TS2304"),
            one("src/app.ts", Some(7), Some(13))
        );
        assert_eq!(refs("main.c:3: warning"), one("main.c", Some(3), None));
        assert_eq!(
            refs(r"C:\work\x\Program.cs(12,5): error CS1002"),
            one(r"C:\work\x\Program.cs", Some(12), Some(5))
        );
        assert_eq!(refs("Foo.cs(8): warning"), one("Foo.cs", Some(8), None));
        assert_eq!(
            refs(r"C:\Erik\work\a.rs:10:2"),
            one(r"C:\Erik\work\a.rs", Some(10), Some(2))
        );
        assert_eq!(
            refs("(see crates/a/lib.rs:3)."),
            one("crates/a/lib.rs", Some(3), None)
        );
        assert_eq!(
            refs("Makedir/Makefile:12"),
            one("Makedir/Makefile", Some(12), None)
        );
    }

    #[test]
    fn agent_forms() {
        let line = "● Update(src/main.rs)";
        let r = file_refs(line);
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!((r[0].path.as_str(), r[0].line), ("src/main.rs", None));
        let chars: Vec<char> = line.chars().collect();
        assert_eq!(
            chars[r[0].start..r[0].end].iter().collect::<String>(),
            "src/main.rs"
        );
        assert_eq!(
            refs("Read(crates/a/b.rs:12-30)"),
            one("crates/a/b.rs", Some(12), None)
        );
        assert_eq!(refs("see src/x.rs:12-30."), one("src/x.rs", Some(12), None));
        // A call without a path inside is nothing.
        assert!(refs("Bash(cargo test)").is_empty());
        assert!(refs("print(x)").is_empty());
    }

    #[test]
    fn python_traceback() {
        let line = r#"  File "C:\My Code\app.py", line 42, in main"#;
        let r = file_refs(line);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].path, r"C:\My Code\app.py");
        assert_eq!(r[0].line, Some(42));
        // The span is the path inside the quotes.
        let chars: Vec<char> = line.chars().collect();
        assert_eq!(
            chars[r[0].start..r[0].end].iter().collect::<String>(),
            r[0].path
        );
    }

    #[test]
    fn bare_paths_need_folder_and_extension() {
        assert_eq!(
            refs("modified:   src/lib.rs"),
            one("src/lib.rs", None, None)
        );
        assert!(refs("see lib.rs for details").is_empty());
        assert!(refs("src/bin and docs/").is_empty());
    }

    #[test]
    fn not_references() {
        for text in [
            "listening on localhost:8080",
            "version 1.2.3 released",
            "ratio 3:4",
            "https://example.com/a.rs:3",
            "time 12:30:45",
            "-o:out.txt",
            "file.rs:0",
            "file.rs:99999999999",
            "[11/Nov/2026:10:30:45 +0100]",
            "ssh to github.com:22 failed",
            "proxy example.org:443",
            r"C:\x\NUL.txt:3",
            "logs/COM1.log:2",
            "a:b/c.rs:3",
        ] {
            assert!(refs(text).is_empty(), "{text}: {:?}", refs(text));
        }
    }

    #[test]
    fn spans_and_lookup() {
        let line = "error: ä src/main.rs:4 and https://x.org/y.rs:5";
        let r = file_ref_at(line, 10).unwrap();
        assert_eq!((r.start, r.end), (9, 20));
        assert_eq!(r.path, "src/main.rs");
        // Inside the web link: that is a link, not a file.
        let link_pos = line.chars().count() - 3;
        assert_eq!(file_ref_at(line, link_pos), None);
        assert_eq!(file_ref_at(line, 0), None);
    }

    #[test]
    fn resolve_relative_absolute_and_unsafe() {
        let root = if cfg!(windows) { r"C:\work" } else { "/work" };
        let base_a = Path::new(root).join("a");
        let base_b = Path::new(root).join("b");
        let target = base_b.join("src/x.rs");
        let exists = |p: &Path| p == target;
        assert_eq!(
            resolve("src/x.rs", &[base_a.clone(), base_b.clone()], exists),
            Some(target.clone())
        );
        assert_eq!(resolve("src/y.rs", &[base_a, base_b], exists), None);
        // A network base (e.g. a shell in a UNC folder) is never touched.
        let touched = std::cell::Cell::new(false);
        let probe = |_: &Path| {
            touched.set(true);
            true
        };
        assert_eq!(
            resolve("x.rs", &[PathBuf::from(r"\\server\share")], probe),
            None
        );
        assert!(!touched.get(), "is_file must not run for a network path");
        // Windows network/device forms. (On Linux `//x` is an ordinary local path
        // and backslashes are file-name characters.)
        if cfg!(windows) {
            for unsafe_path in [
                r"\\server\share\x.rs",
                "//server/share/x.rs",
                r"\\?\C:\x.rs",
                r"\\?\UNC\server\share\x.rs",
            ] {
                assert_eq!(resolve(unsafe_path, &[], |_| true), None, "{unsafe_path}");
            }
        }
        let abs = if cfg!(windows) {
            r"C:\x\y.rs"
        } else {
            "/x/y.rs"
        };
        assert_eq!(resolve(abs, &[], |_| true), Some(PathBuf::from(abs)));
        if cfg!(windows) {
            // Rooted without a drive: unknown drive, not opened.
            assert_eq!(
                resolve(r"\x\y.rs", &[PathBuf::from(r"C:\b")], |_| true),
                None
            );
        }
    }
}
