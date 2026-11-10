//! Literal text search on top of alacritty's regex search.

/// Escapes `text` so the regex engine matches it literally: every ASCII
/// punctuation character gets a backslash (valid for all of them in Rust regex
/// syntax), everything else stays as is.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for c in text.chars() {
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_regex_syntax() {
        assert_eq!(escape("a.b"), r"a\.b");
        assert_eq!(escape("C:\\x (1)"), r"C\:\\x \(1\)");
        assert_eq!(escape("[*]+?"), r"\[\*\]\+\?");
        assert_eq!(escape("héllo wörld"), "héllo wörld");
        assert_eq!(escape(""), "");
    }
}
