//! Code viewer backend: loads a text file safely (size limit, binary detection,
//! UTF-8 / UTF-16 with BOM, line endings) and highlights it with syntect using a
//! theme built from the Forge design tokens. Read-only: nothing is ever written.

use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::OnceLock;
use std::time::SystemTime;

use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings,
};
use syntect::parsing::SyntaxSet;

/// Files larger than this are not opened (a viewer is not a hex editor).
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;
/// Above these sizes the text is shown without colors (highlighting would take long).
pub const MAX_HIGHLIGHT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_HIGHLIGHT_LINES: usize = 50_000;
/// Columns a tab expands to.
pub const TAB_WIDTH: usize = 4;
/// Longer lines are cut for display (the copy keeps the full text).
pub const MAX_DISPLAY_COLUMNS: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    /// Not valid UTF-8: shown with replacement characters.
    Lossy,
}

impl Encoding {
    pub fn label(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 BOM",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
            Encoding::Lossy => "not valid UTF-8",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub path: PathBuf,
    /// Lines as in the file (without line ending), for copying.
    pub lines: Vec<String>,
    /// Lines for display: tabs expanded, control characters made visible, cut at
    /// [`MAX_DISPLAY_COLUMNS`].
    pub display: Vec<String>,
    pub encoding: Encoding,
    /// The file uses CRLF line endings (at least one line).
    pub crlf: bool,
    pub bytes: u64,
    pub modified: Option<SystemTime>,
    /// Syntax name used for highlighting ("Plain Text" when unknown).
    pub syntax: String,
}

impl Document {
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Whether the whole file can be highlighted in reasonable time.
    pub fn highlightable(&self) -> bool {
        self.bytes as usize <= MAX_HIGHLIGHT_BYTES && self.lines.len() <= MAX_HIGHLIGHT_LINES
    }
}

/// Last modification time, for change detection.
pub fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

pub fn load(path: &Path) -> Result<Document, String> {
    let meta = fs::metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if meta.is_dir() {
        return Err(format!("{} is a folder", path.display()));
    }
    if meta.len() > MAX_BYTES {
        return Err(format!(
            "{} is {:.1} MB; the viewer opens files up to {} MB",
            file_name(path),
            meta.len() as f64 / 1_048_576.0,
            MAX_BYTES / 1_048_576
        ));
    }
    let bytes = fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let (text, encoding) =
        decode(&bytes).ok_or_else(|| format!("{} looks like a binary file", file_name(path)))?;
    let mut crlf = false;
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|l| match l.strip_suffix('\r') {
            Some(l) => {
                crlf = true;
                l.to_owned()
            }
            None => l.to_owned(),
        })
        .collect();
    // A final newline ends the last line; it does not start an empty one.
    if lines.len() > 1 && lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    let display = lines.iter().map(|l| display_line(l)).collect();
    let syntax = syntax_for(path, lines.first().map(String::as_str))
        .map_or_else(|| "Plain Text".to_owned(), |s| s.name.clone());
    Ok(Document {
        path: path.to_path_buf(),
        lines,
        display,
        encoding,
        crlf,
        bytes: meta.len(),
        modified: meta.modified().ok(),
        syntax,
    })
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Text and encoding, or `None` for binary data (NUL bytes without a UTF-16 BOM).
pub fn decode(bytes: &[u8]) -> Option<(String, Encoding)> {
    let utf16 = |rest: &[u8], le: bool| -> String {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| {
                if le {
                    u16::from_le_bytes([c[0], c[1]])
                } else {
                    u16::from_be_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return Some((utf16(rest, true), Encoding::Utf16Le));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return Some((utf16(rest, false), Encoding::Utf16Be));
    }
    if bytes.iter().take(8192).any(|&b| b == 0) {
        return None;
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Some((
            String::from_utf8_lossy(rest).into_owned(),
            Encoding::Utf8Bom,
        ));
    }
    match String::from_utf8(bytes.to_vec()) {
        Ok(s) => Some((s, Encoding::Utf8)),
        Err(e) => Some((
            String::from_utf8_lossy(e.as_bytes()).into_owned(),
            Encoding::Lossy,
        )),
    }
}

/// Tabs to spaces (to the next multiple of [`TAB_WIDTH`]), other control characters
/// to their visible Unicode symbol (U+2400 block), cut at [`MAX_DISPLAY_COLUMNS`].
pub fn display_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut col = 0;
    for c in line.chars() {
        if col >= MAX_DISPLAY_COLUMNS {
            out.push('…');
            break;
        }
        match c {
            '\t' => {
                let n = TAB_WIDTH - col % TAB_WIDTH;
                out.extend(std::iter::repeat_n(' ', n));
                col += n;
            }
            c if (c as u32) < 0x20 => {
                out.push(char::from_u32(0x2400 + c as u32).unwrap_or('�'));
                col += 1;
            }
            '\u{7f}' => {
                out.push('\u{2421}');
                col += 1;
            }
            c => {
                out.push(c);
                col += 1;
            }
        }
    }
    out
}

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn syntax_for<'a>(
    path: &Path,
    first_line: Option<&str>,
) -> Option<&'a syntect::parsing::SyntaxReference> {
    let set = syntaxes();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase());
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    // A few common files whose extension the default set does not know.
    let alias = match (name.as_deref(), ext.as_deref()) {
        (Some("makefile" | "gnumakefile"), _) => Some("make"),
        (Some("dockerfile"), _) => Some("sh"),
        (_, Some("jsonc" | "json5")) => Some("json"),
        (_, Some("mjs" | "cjs")) => Some("js"),
        (_, Some("bashrc" | "zsh")) => Some("sh"),
        _ => None,
    };
    alias
        .and_then(|a| set.find_syntax_by_extension(a))
        .or_else(|| ext.as_deref().and_then(|e| set.find_syntax_by_extension(e)))
        .or_else(|| first_line.and_then(|l| set.find_syntax_by_first_line(l)))
        .filter(|s| s.name != "Plain Text")
}

/// A colored run of a display line (byte range into `Document::display[line]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub color: Rgb,
    pub bold: bool,
    pub italic: bool,
}

/// Highlights every display line. `None` when the file has no known syntax or is
/// too large (see [`Document::highlightable`]). Runs for a while on big files: call
/// it off the UI thread.
pub fn highlight(doc: &Document) -> Option<Vec<Vec<Span>>> {
    if !doc.highlightable() {
        return None;
    }
    let first = doc.lines.first().map(String::as_str);
    let syntax = syntax_for(&doc.path, first)?;
    let theme = forge_theme();
    let set = syntaxes();
    let mut h = HighlightLines::new(syntax, theme);
    let mut out = Vec::with_capacity(doc.display.len());
    let mut buf = String::new();
    for line in &doc.display {
        buf.clear();
        buf.push_str(line);
        buf.push('\n');
        let ranges = h.highlight_line(&buf, set).ok()?;
        let mut spans = Vec::new();
        let mut pos = 0;
        for (style, piece) in ranges {
            let start = pos;
            pos += piece.len();
            let end = pos.min(line.len());
            if start >= end || piece.trim().is_empty() {
                continue;
            }
            let c = style.foreground;
            spans.push(Span {
                start,
                end,
                color: Rgb {
                    r: c.r,
                    g: c.g,
                    b: c.b,
                },
                bold: style.font_style.contains(FontStyle::BOLD),
                italic: style.font_style.contains(FontStyle::ITALIC),
            });
        }
        out.push(spans);
    }
    Some(out)
}

/// Default text color of the theme (the color of unhighlighted text).
pub const TEXT: Rgb = Rgb {
    r: 0xD6,
    g: 0xDE,
    b: 0xEB,
};

/// Forge Dark for code: the design tokens mapped onto TextMate scopes.
fn forge_theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| {
        let c = |hex: u32| Color {
            r: (hex >> 16) as u8,
            g: (hex >> 8) as u8,
            b: hex as u8,
            a: 0xFF,
        };
        let item = |scopes: &str, color: u32, style: Option<FontStyle>| ThemeItem {
            scope: ScopeSelectors::from_str(scopes).expect("valid scope selector"),
            style: StyleModifier {
                foreground: Some(c(color)),
                background: None,
                font_style: style,
            },
        };
        Theme {
            name: Some("Forge Dark".into()),
            author: Some("Erik van de Blaak".into()),
            settings: ThemeSettings {
                foreground: Some(c(0xD6DEEB)),
                background: Some(c(0x090D14)),
                ..ThemeSettings::default()
            },
            scopes: vec![
                item(
                    "comment, punctuation.definition.comment",
                    0x6B7890,
                    Some(FontStyle::ITALIC),
                ),
                item(
                    "string, string.quoted, punctuation.definition.string",
                    0x55E6B5,
                    None,
                ),
                item(
                    "constant.numeric, constant.language, constant.character",
                    0xF5C978,
                    None,
                ),
                item("constant.other, variable.other.constant", 0xF5C978, None),
                item(
                    "keyword, storage, storage.type, storage.modifier",
                    0xA78BFA,
                    None,
                ),
                item("keyword.operator", 0xC9D3E3, None),
                item(
                    "entity.name.function, support.function, meta.function-call",
                    0x4DD8E8,
                    None,
                ),
                item(
                    "entity.name.type, entity.name.class, support.type, support.class",
                    0x7CC4FA,
                    None,
                ),
                item("entity.name.tag", 0xF08080, None),
                item("entity.other.attribute-name", 0xF5C978, None),
                item("variable.parameter", 0xE6B8A2, None),
                item("entity.name.namespace, entity.name.module", 0x7CC4FA, None),
                item("markup.heading", 0x4DD8E8, Some(FontStyle::BOLD)),
                item("markup.bold", 0xD6DEEB, Some(FontStyle::BOLD)),
                item("markup.italic", 0xD6DEEB, Some(FontStyle::ITALIC)),
                item("markup.inserted", 0x55E6B5, None),
                item("markup.deleted", 0xF08080, None),
                item("invalid", 0xF08080, None),
            ],
        }
    })
}

/// Lines `from..=to` (clamped) of the original text, joined with `\n`.
pub fn copy_lines(doc: &Document, from: usize, to: usize) -> String {
    let (a, b) = if from <= to { (from, to) } else { (to, from) };
    let b = b.min(doc.lines.len().saturating_sub(1));
    doc.lines
        .get(a..=b)
        .map(|l| l.join("\n"))
        .unwrap_or_default()
}

/// Line numbers (0-based) containing `needle`, case-insensitive for ASCII.
pub fn find(doc: &Document, needle: &str) -> Vec<usize> {
    if needle.is_empty() {
        return Vec::new();
    }
    let lower = needle.to_ascii_lowercase();
    doc.lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.to_ascii_lowercase().contains(&lower))
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("forgeterm-viewer-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn decodes_utf8_bom_utf16_and_detects_binary() {
        assert_eq!(decode(b"hi").unwrap(), ("hi".into(), Encoding::Utf8));
        assert_eq!(
            decode(b"\xEF\xBB\xBFhi").unwrap(),
            ("hi".into(), Encoding::Utf8Bom)
        );
        assert_eq!(
            decode(b"\xFF\xFEh\0i\0").unwrap(),
            ("hi".into(), Encoding::Utf16Le)
        );
        assert_eq!(
            decode(b"\xFE\xFF\0h\0i").unwrap(),
            ("hi".into(), Encoding::Utf16Be)
        );
        assert_eq!(decode(b"a\0b"), None);
        let (text, enc) = decode(b"caf\xE9").unwrap();
        assert_eq!(enc, Encoding::Lossy);
        assert!(text.starts_with("caf"));
    }

    #[test]
    fn display_lines_expand_tabs_and_show_control_characters() {
        assert_eq!(display_line("\tx"), "    x");
        assert_eq!(display_line("ab\tc"), "ab  c");
        // An escape sequence in a file is shown, never interpreted.
        assert_eq!(display_line("\u{1b}[31m"), "\u{241b}[31m");
        assert_eq!(display_line("a\u{7f}"), "a\u{2421}");
        let long = "x".repeat(MAX_DISPLAY_COLUMNS + 10);
        assert_eq!(display_line(&long).chars().count(), MAX_DISPLAY_COLUMNS + 1);
    }

    #[test]
    fn loads_lines_with_crlf_and_final_newline() {
        let p = temp("a.rs", b"fn main() {\r\n\tlet x = 1;\r\n}\r\n");
        let d = load(&p).unwrap();
        assert_eq!(d.lines, ["fn main() {", "\tlet x = 1;", "}"]);
        assert_eq!(d.display[1], "    let x = 1;");
        assert!(d.crlf);
        assert_eq!(d.syntax, "Rust");
        assert_eq!(copy_lines(&d, 2, 1), "\tlet x = 1;\n}");
        assert_eq!(copy_lines(&d, 0, 99), "fn main() {\n\tlet x = 1;\n}");
        assert_eq!(find(&d, "LET"), [1]);
        assert!(find(&d, "").is_empty());

        // Empty file: one empty line.
        let d = load(&temp("empty.txt", b"")).unwrap();
        assert_eq!(d.lines, [""]);
        assert_eq!(d.syntax, "Plain Text");
    }

    #[test]
    fn refuses_binary_folders_and_huge_files() {
        assert!(
            load(&temp("bin.dat", b"\x00\x01\x02"))
                .unwrap_err()
                .contains("binary")
        );
        let dir = std::env::temp_dir();
        assert!(load(&dir).unwrap_err().contains("folder"));
        assert!(load(&dir.join("does-not-exist-forgeterm.txt")).is_err());
        let big = temp("big.txt", &vec![b'a'; (MAX_BYTES + 1) as usize]);
        assert!(load(&big).unwrap_err().contains("MB"));
        let _ = fs::remove_file(big);
    }

    #[test]
    fn highlights_rust_with_forge_colors() {
        let p = temp("h.rs", b"// note\nfn main() { let s = \"hi\"; }\n");
        let d = load(&p).unwrap();
        let spans = highlight(&d).unwrap();
        assert_eq!(spans.len(), 2);
        let color_of = |line: usize, text: &str| {
            let start = d.display[line].find(text).unwrap();
            spans[line]
                .iter()
                .find(|s| s.start <= start && start < s.end)
                .map(|s| s.color)
        };
        let rgb = |hex: u32| Rgb {
            r: (hex >> 16) as u8,
            g: (hex >> 8) as u8,
            b: hex as u8,
        };
        assert_eq!(color_of(0, "note"), Some(rgb(0x6B7890)), "comment");
        assert_eq!(color_of(1, "fn"), Some(rgb(0xA78BFA)), "keyword");
        assert_eq!(color_of(1, "hi"), Some(rgb(0x55E6B5)), "string");
        // Spans stay inside the display line.
        for (i, line) in spans.iter().enumerate() {
            for s in line {
                assert!(s.end <= d.display[i].len());
                assert!(d.display[i].is_char_boundary(s.start));
            }
        }
        // Unknown syntax: no highlighting.
        assert!(highlight(&load(&temp("x.unknownext", b"text")).unwrap()).is_none());
    }

    #[test]
    fn known_files_get_a_syntax() {
        for (name, want) in [
            ("x.py", "Python"),
            ("x.json", "JSON"),
            ("x.md", "Markdown"),
            ("Makefile", "Makefile"),
            ("x.sh", "Bourne Again Shell (bash)"),
        ] {
            let d = load(&temp(name, b"x\n")).unwrap();
            assert_eq!(d.syntax, want, "{name}");
        }
        // Shebang on the first line.
        let d = load(&temp("script", b"#!/usr/bin/env python3\nprint(1)\n")).unwrap();
        assert_eq!(d.syntax, "Python");
    }
}
