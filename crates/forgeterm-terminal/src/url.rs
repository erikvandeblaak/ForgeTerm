//! Finding web links in terminal text (for Ctrl+click). Only `http://` and
//! `https://`: other schemes (`file:`, custom protocol handlers) could start
//! programs, and terminal output is untrusted.

/// Longest link accepted.
pub const MAX_URL_LEN: usize = 2048;

/// A link found in a line: char range `[start, end)` and the link text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub start: usize,
    pub end: usize,
    pub url: String,
}

/// The link covering char index `at` in `line`, if any.
pub fn link_at(line: &str, at: usize) -> Option<Link> {
    links(line)
        .into_iter()
        .find(|l| l.start <= at && at < l.end)
}

/// All links in `line`, left to right.
pub fn links(line: &str) -> Vec<Link> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let rest: String = chars[i..chars.len().min(i + 8)].iter().collect();
        let scheme_len = if rest.to_ascii_lowercase().starts_with("https://") {
            8
        } else if rest.to_ascii_lowercase().starts_with("http://") {
            7
        } else {
            i += 1;
            continue;
        };
        // A scheme glued to a word (`xhttp://`) is not a link start.
        if i > 0 && chars[i - 1].is_alphanumeric() {
            i += 1;
            continue;
        }
        let mut end = i + scheme_len;
        while end < chars.len() && is_url_char(chars[end]) {
            end += 1;
        }
        end = trim_trailing(&chars, i, end);
        if end > i + scheme_len && end - i <= MAX_URL_LEN {
            out.push(Link {
                start: i,
                end,
                url: chars[i..end].iter().collect(),
            });
        }
        i = end.max(i + 1);
    }
    out
}

fn is_url_char(c: char) -> bool {
    !(c.is_whitespace()
        || c.is_control()
        || matches!(
            c,
            '<' | '>' | '"' | '\'' | '`' | '{' | '}' | '|' | '\\' | '^'
        ))
}

/// Drops sentence punctuation after a link and closing brackets that have no
/// opening partner inside it (`(see https://x.org/a)` → `https://x.org/a`).
fn trim_trailing(chars: &[char], start: usize, mut end: usize) -> usize {
    loop {
        let Some(&last) = chars.get(end.wrapping_sub(1)).filter(|_| end > start) else {
            return end;
        };
        let unbalanced = |open: char, close: char| {
            let s = &chars[start..end];
            s.iter().filter(|&&c| c == close).count() > s.iter().filter(|&&c| c == open).count()
        };
        let drop = match last {
            '.' | ',' | ';' | ':' | '!' | '?' | '*' => true,
            ')' => unbalanced('(', ')'),
            ']' => unbalanced('[', ']'),
            _ => false,
        };
        if !drop {
            return end;
        }
        end -= 1;
    }
}

/// Whether `url` is safe to hand to the system's browser: http(s), a host,
/// printable ASCII-or-Unicode without spaces, quotes or control characters.
pub fn is_openable(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let rest = if let Some(r) = lower.strip_prefix("https://") {
        r
    } else if let Some(r) = lower.strip_prefix("http://") {
        r
    } else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    url.len() <= MAX_URL_LEN
        && !host.is_empty()
        && !host.starts_with('-')
        && url.chars().all(is_url_char)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(line: &str) -> Vec<String> {
        links(line).into_iter().map(|l| l.url).collect()
    }

    #[test]
    fn finds_links_and_trims_punctuation() {
        assert_eq!(
            urls("see https://example.com/a?b=1#c."),
            ["https://example.com/a?b=1#c"]
        );
        assert_eq!(
            urls("(docs: https://x.org/wiki/Rust_(language))"),
            ["https://x.org/wiki/Rust_(language)"]
        );
        assert_eq!(urls("(see https://x.org/a)"), ["https://x.org/a"]);
        assert_eq!(
            urls("two http://a.io, https://b.io!"),
            ["http://a.io", "https://b.io"]
        );
        assert_eq!(
            urls("<https://in.angle/brackets>"),
            ["https://in.angle/brackets"]
        );
        assert_eq!(urls("HTTPS://UPPER.CASE/x"), ["HTTPS://UPPER.CASE/x"]);
        assert_eq!(urls("ünïcode → https://ex.am/ple"), ["https://ex.am/ple"]);
    }

    #[test]
    fn ignores_non_links() {
        assert!(urls("no links here").is_empty());
        assert!(urls("https:// alone").is_empty());
        assert!(urls("file:///C:/Windows/system32").is_empty());
        assert!(urls("ftp://example.com").is_empty());
        assert!(urls("javascript:alert(1)").is_empty());
        assert!(urls("xhttps://glued.example").is_empty());
    }

    #[test]
    fn link_at_uses_char_positions() {
        let line = "ab https://é.example/x cd";
        let l = link_at(line, 5).unwrap();
        assert_eq!((l.start, l.end), (3, 22));
        assert_eq!(link_at(line, 2), None);
        assert_eq!(link_at(line, 22), None);
    }

    #[test]
    fn only_safe_urls_are_openable() {
        assert!(is_openable("https://example.com/path?q=1"));
        assert!(is_openable("http://localhost:8080"));
        assert!(!is_openable("https://"));
        assert!(!is_openable("https://-flag"));
        assert!(!is_openable("file:///etc/passwd"));
        assert!(!is_openable("https://a.b/\"&calc"));
        assert!(!is_openable("https://a.b/x y"));
        assert!(!is_openable("https://a.b/\u{1b}[31m"));
        assert!(!is_openable(&format!(
            "https://a.b/{}",
            "x".repeat(MAX_URL_LEN)
        )));
    }
}
