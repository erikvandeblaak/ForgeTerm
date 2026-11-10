//! Quoting file paths for insertion into a specific shell.
//!
//! The result is meant to be *inserted* at the prompt (never executed by ForgeTerm):
//! it must reach the program as exactly one argument, whatever characters the path
//! contains. Windows paths are converted for shells that expect POSIX paths
//! (Git Bash: `/c/...`, WSL: `/mnt/c/...`).

use crate::shell::ShellKind;

/// Characters that never need quoting in any supported shell.
fn is_plain(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '/' | ':')
}

/// Quotes one path for `kind`.
pub fn quote_path(kind: ShellKind, path: &str) -> String {
    let path = strip_verbatim(path);
    match kind {
        ShellKind::PowerShell => quote_powershell(&path),
        ShellKind::Cmd => quote_cmd(&path),
        ShellKind::Posix => quote_posix(&path),
        ShellKind::Fish => quote_fish(&path),
        ShellKind::GitBash => quote_posix(&windows_to_msys(&path)),
        ShellKind::Wsl => quote_posix(&windows_to_wsl(&path)),
    }
}

/// Quotes several paths, separated by single spaces.
pub fn quote_paths<S: AsRef<str>>(kind: ShellKind, paths: &[S]) -> String {
    paths
        .iter()
        .map(|p| quote_path(kind, p.as_ref()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `\\?\C:\x` → `C:\x`, `\\?\UNC\server\share` → `\\server\share` (what
/// `std::fs::canonicalize` returns on Windows; shells do not understand it).
fn strip_verbatim(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        path.to_owned()
    }
}

fn quote_powershell(path: &str) -> String {
    if !path.is_empty() && path.chars().all(|c| is_plain(c) || c == '\\') {
        return path.to_owned();
    }
    // Single-quoted strings are literal; PowerShell treats all four single-quote
    // characters as delimiters and escapes each by doubling it.
    let mut out = String::with_capacity(path.len() + 2);
    out.push('\'');
    for c in path.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
}

fn quote_cmd(path: &str) -> String {
    if !path.is_empty() && path.chars().all(|c| is_plain(c) || c == '\\') {
        return path.to_owned();
    }
    // `"` cannot occur in Windows file names, so plain double quotes suffice.
    format!("\"{path}\"")
}

fn quote_posix(path: &str) -> String {
    if !path.is_empty() && path.chars().all(is_plain) {
        return path.to_owned();
    }
    format!("'{}'", path.replace('\'', r"'\''"))
}

fn quote_fish(path: &str) -> String {
    if !path.is_empty() && path.chars().all(is_plain) {
        return path.to_owned();
    }
    // In fish single quotes, only \' and \\ are escapes.
    format!("'{}'", path.replace('\\', r"\\").replace('\'', r"\'"))
}

/// Splits `C:\a\b` into ('c', "a/b"); `None` if it is not a drive path.
fn drive_path(path: &str) -> Option<(char, String)> {
    let mut chars = path.chars();
    let drive = chars.next().filter(char::is_ascii_alphabetic)?;
    if chars.next() != Some(':') {
        return None;
    }
    let rest = chars.as_str();
    if !(rest.is_empty() || rest.starts_with(['\\', '/'])) {
        return None;
    }
    let rest = rest.trim_start_matches(['\\', '/']).replace('\\', "/");
    Some((drive.to_ascii_lowercase(), rest))
}

fn windows_to_msys(path: &str) -> String {
    if let Some((drive, rest)) = drive_path(path) {
        return format!("/{drive}/{rest}");
    }
    if path.starts_with(r"\\") {
        return path.replace('\\', "/");
    }
    path.to_owned()
}

fn windows_to_wsl(path: &str) -> String {
    match drive_path(path) {
        Some((drive, rest)) if rest.is_empty() => format!("/mnt/{drive}"),
        Some((drive, rest)) => format!("/mnt/{drive}/{rest}"),
        // UNC and relative paths have no fixed WSL equivalent; pass them unchanged.
        None => path.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ShellKind::*;

    #[test]
    fn plain_paths_stay_unquoted() {
        assert_eq!(quote_path(PowerShell, r"C:\Erik\work"), r"C:\Erik\work");
        assert_eq!(quote_path(Cmd, r"C:\Erik\work"), r"C:\Erik\work");
        assert_eq!(quote_path(Posix, "/home/erik/src"), "/home/erik/src");
        assert_eq!(quote_path(Fish, "/tmp/a.txt"), "/tmp/a.txt");
    }

    #[test]
    fn powershell_single_quotes_and_doubles_quote_chars() {
        assert_eq!(
            quote_path(PowerShell, r"C:\My Files\it's $HOME.txt"),
            r"'C:\My Files\it''s $HOME.txt'"
        );
        // Typographic quotes also end a PowerShell string.
        assert_eq!(
            quote_path(PowerShell, "C:\\a\u{2019}b"),
            "'C:\\a\u{2019}\u{2019}b'"
        );
        assert_eq!(quote_path(PowerShell, "C:\\a (1)&b"), "'C:\\a (1)&b'");
    }

    #[test]
    fn cmd_double_quotes() {
        assert_eq!(
            quote_path(Cmd, r"C:\Program Files\a&b"),
            r#""C:\Program Files\a&b""#
        );
    }

    #[test]
    fn posix_quotes_survive_any_character() {
        assert_eq!(quote_path(Posix, "/tmp/it's here"), r"'/tmp/it'\''s here'");
        assert_eq!(quote_path(Posix, "/tmp/$(rm -rf ~)"), "'/tmp/$(rm -rf ~)'");
        assert_eq!(quote_path(Posix, "/tmp/a\\b"), "'/tmp/a\\b'");
        assert_eq!(quote_path(Posix, "~/x"), "'~/x'");
        assert_eq!(quote_path(Posix, ""), "''");
    }

    #[test]
    fn fish_escapes_backslash_and_quote() {
        assert_eq!(quote_path(Fish, r"/tmp/it's a\b"), r"'/tmp/it\'s a\\b'");
    }

    #[test]
    fn git_bash_gets_msys_paths() {
        assert_eq!(
            quote_path(GitBash, r"C:\Users\Erik\My Docs"),
            "'/c/Users/Erik/My Docs'"
        );
        assert_eq!(quote_path(GitBash, r"D:\src"), "/d/src");
        assert_eq!(quote_path(GitBash, r"C:\"), "/c/");
        assert_eq!(quote_path(GitBash, r"\\server\share\x"), "//server/share/x");
    }

    #[test]
    fn wsl_gets_mnt_paths() {
        assert_eq!(quote_path(Wsl, r"C:\Users\Erik"), "/mnt/c/Users/Erik");
        assert_eq!(quote_path(Wsl, r"E:\"), "/mnt/e");
        assert_eq!(quote_path(Wsl, r"C:\a b"), "'/mnt/c/a b'");
    }

    #[test]
    fn verbatim_prefix_is_removed() {
        assert_eq!(quote_path(PowerShell, r"\\?\C:\x"), r"C:\x");
        assert_eq!(quote_path(Cmd, r"\\?\UNC\srv\share"), r"\\srv\share");
    }

    #[test]
    fn multiple_paths_are_space_separated() {
        assert_eq!(
            quote_paths(PowerShell, &[r"C:\a", r"C:\b c"]),
            r"C:\a 'C:\b c'"
        );
    }
}
