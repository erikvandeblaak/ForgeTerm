//! Prompt Studio templates: Markdown files with an optional front matter
//! (`name`, `description`) and `{{variable}}` placeholders.
//!
//! Built-in templates ship with ForgeTerm; the user's own templates are `.md` files
//! in a folder next to `settings.toml`, and override a built-in with the same file
//! name. Nothing here sends or runs anything: rendering only produces text, which
//! the GUI inserts without Enter.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Variables ForgeTerm fills in from real state (still editable in the studio).
pub const AUTO_VARIABLES: [&str; 5] = ["project", "project_path", "cwd", "file", "selection"];

/// Largest template file read (anything bigger is reported, not loaded).
pub const MAX_FILE_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Builtin,
    User(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// File stem, e.g. `review-file`; a user file with the same id replaces a built-in.
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub body: String,
    pub source: Source,
}

impl Template {
    /// Parses a template file. The front matter is optional; without a `name` the
    /// id is used.
    pub fn parse(id: &str, text: &str, source: Source) -> Template {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let (meta, body) = split_front_matter(text);
        let get = |key: &str| {
            meta.iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
                .filter(|v| !v.is_empty())
        };
        Template {
            id: id.to_owned(),
            name: get("name").unwrap_or_else(|| id.to_owned()),
            description: get("description"),
            body: body.trim_matches(['\r', '\n']).to_owned(),
            source,
        }
    }

    /// Variable names in order of first appearance.
    pub fn variables(&self) -> Vec<String> {
        variables(&self.body)
    }

    pub fn is_builtin(&self) -> bool {
        self.source == Source::Builtin
    }
}

/// `---` front matter as `key: value` pairs, and the rest of the text.
fn split_front_matter(text: &str) -> (Vec<(String, String)>, &str) {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return (Vec::new(), text);
    };
    let mut meta = Vec::new();
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        offset += line.len();
        let line = line.trim_end_matches(['\r', '\n']);
        if line.trim_end() == "---" {
            return (meta, &rest[offset..]);
        }
        if let Some((k, v)) = line.split_once(':') {
            let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
            meta.push((k.trim().to_owned(), v.to_owned()));
        }
    }
    // No closing marker: not front matter after all.
    (Vec::new(), text)
}

/// One `{{name}}` occurrence: byte range in the body and the variable name.
fn placeholders(body: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(start) = body[from..].find("{{").map(|i| from + i) {
        let Some(len) = body[start + 2..].find("}}") else {
            break;
        };
        let inner = &body[start + 2..start + 2 + len];
        let name = inner.trim();
        let valid = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if valid {
            out.push((start..start + 2 + len + 2, name));
            from = start + 2 + len + 2;
        } else {
            // Not a placeholder (e.g. `{{ a b }}` or `{{{`): skip one brace.
            from = start + 1;
        }
    }
    out
}

/// Variable names in `body`, in order of first appearance, without duplicates.
pub fn variables(body: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (_, name) in placeholders(body) {
        if !names.iter().any(|n| n == name) {
            names.push(name.to_owned());
        }
    }
    names
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub text: String,
    /// Variables without a (non-blank) value; they stay as `{{name}}` in `text`.
    pub missing: Vec<String>,
}

/// Fills in the placeholders. Values are inserted literally (never re-scanned for
/// placeholders). Trailing whitespace and newlines are removed so the result never
/// ends with an Enter.
pub fn render(body: &str, values: &HashMap<String, String>) -> Rendered {
    let mut text = String::with_capacity(body.len());
    let mut missing: Vec<String> = Vec::new();
    let mut last = 0;
    for (range, name) in placeholders(body) {
        text.push_str(&body[last..range.start]);
        match values.get(name).filter(|v| !v.trim().is_empty()) {
            Some(value) => text.push_str(value),
            None => {
                text.push_str(&body[range.clone()]);
                if !missing.iter().any(|m| m == name) {
                    missing.push(name.to_owned());
                }
            }
        }
        last = range.end;
    }
    text.push_str(&body[last..]);
    let text = text.trim_end().to_owned();
    Rendered { text, missing }
}

const BUILTIN: [(&str, &str); 6] = [
    ("explain-error", include_str!("../builtin/explain-error.md")),
    ("review-file", include_str!("../builtin/review-file.md")),
    ("explain-code", include_str!("../builtin/explain-code.md")),
    ("write-tests", include_str!("../builtin/write-tests.md")),
    ("plan-feature", include_str!("../builtin/plan-feature.md")),
    (
        "commit-message",
        include_str!("../builtin/commit-message.md"),
    ),
];

pub fn builtin() -> Vec<Template> {
    BUILTIN
        .iter()
        .map(|(id, text)| Template::parse(id, text, Source::Builtin))
        .collect()
}

/// The user's templates folder for a given settings file: `<settings dir>/prompts`.
pub fn user_dir(settings_file: &Path) -> Option<PathBuf> {
    settings_file.parent().map(|d| d.join("prompts"))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Library {
    /// User templates first (sorted by name), then the remaining built-ins.
    pub templates: Vec<Template>,
    /// Files that could not be read, as text for the UI.
    pub problems: Vec<String>,
}

/// Built-ins plus the `.md` files in `dir` (not recursive). A missing folder is not
/// a problem: it just means the user has no templates yet.
pub fn load(dir: Option<&Path>) -> Library {
    let mut user = Vec::new();
    let mut problems = Vec::new();
    if let Some(dir) = dir
        && let Ok(entries) = fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_md = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("md"));
            if !is_md || !path.is_file() {
                continue;
            }
            let Some(id) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
                continue;
            };
            let name = path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            if entry.metadata().is_ok_and(|m| m.len() > MAX_FILE_BYTES) {
                problems.push(format!(
                    "{name}: larger than {} KB, skipped",
                    MAX_FILE_BYTES / 1024
                ));
                continue;
            }
            match fs::read(&path) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(text) => user.push(Template::parse(&id, &text, Source::User(path))),
                    Err(_) => problems.push(format!("{name}: not UTF-8 text, skipped")),
                },
                Err(err) => problems.push(format!("{name}: {err}")),
            }
        }
    }
    user.sort_by_key(|t| t.name.to_lowercase());
    problems.sort();
    let mut templates = user;
    for b in builtin() {
        if !templates.iter().any(|t| t.id.eq_ignore_ascii_case(&b.id)) {
            templates.push(b);
        }
    }
    Library {
        templates,
        problems,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn parses_front_matter_and_body() {
        let t = Template::parse(
            "x",
            "---\nname: Review\ndescription: \"Find bugs\"\n---\n\nReview {{file}}.\n",
            Source::Builtin,
        );
        assert_eq!(t.name, "Review");
        assert_eq!(t.description.as_deref(), Some("Find bugs"));
        assert_eq!(t.body, "Review {{file}}.");
    }

    #[test]
    fn without_front_matter_the_id_is_the_name() {
        let t = Template::parse("my-prompt", "\u{feff}Hello {{ who }}\r\n", Source::Builtin);
        assert_eq!(t.name, "my-prompt");
        assert_eq!(t.description, None);
        assert_eq!(t.body, "Hello {{ who }}");
        assert_eq!(t.variables(), ["who"]);
        // An unterminated front matter is body text.
        let t = Template::parse("y", "---\nname: x\nno end", Source::Builtin);
        assert_eq!(t.name, "y");
        assert!(t.body.starts_with("---"));
    }

    #[test]
    fn finds_variables_in_order_without_duplicates() {
        assert_eq!(
            variables("{{b}} and {{ a }} then {{b}}, not {{a b}} or {{}} or {{x"),
            ["b", "a"]
        );
        assert_eq!(variables("{{{name}}}"), ["name"]);
        assert!(variables("plain {braces} {{ }}").is_empty());
    }

    #[test]
    fn renders_values_literally_and_reports_missing() {
        let r = render(
            "Fix {{file}} in {{project}}; {{file}} again. {{note}}\n\n",
            &values(&[
                ("file", "src/{{project}}.rs"),
                ("project", "Forge"),
                ("note", "  "),
            ]),
        );
        // The value's own braces are not expanded; blank counts as missing.
        assert_eq!(
            r.text,
            "Fix src/{{project}}.rs in Forge; src/{{project}}.rs again. {{note}}"
        );
        assert_eq!(r.missing, ["note"]);
    }

    #[test]
    fn rendered_text_never_ends_with_a_newline() {
        let r = render("line one\nline two {{x}}", &values(&[("x", "value\r\n\n")]));
        assert_eq!(r.text, "line one\nline two value");
        assert!(r.missing.is_empty());
    }

    #[test]
    fn builtins_parse_with_names_and_variables() {
        let all = builtin();
        assert_eq!(all.len(), BUILTIN.len());
        for t in &all {
            assert_ne!(t.name, t.id, "{} needs a name", t.id);
            assert!(t.description.is_some(), "{} needs a description", t.id);
            assert!(!t.variables().is_empty(), "{} has no variables", t.id);
            assert!(!t.body.ends_with('\n'));
        }
    }

    #[test]
    fn loads_user_templates_that_override_builtins() {
        let dir = std::env::temp_dir().join(format!("forgeterm-prompts-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(
            dir.join("review-file.md"),
            "---\nname: My review\n---\nCheck {{file}}",
        )
        .unwrap();
        fs::write(dir.join("zeta.MD"), "Zeta {{x}}").unwrap();
        fs::write(dir.join("notes.txt"), "ignored").unwrap();
        fs::write(dir.join("sub").join("deep.md"), "not loaded").unwrap();
        fs::write(dir.join("bad.md"), [0xff, 0xfe, 0x00]).unwrap();

        let lib = load(Some(&dir));
        let ids: Vec<&str> = lib.templates.iter().map(|t| t.id.as_str()).collect();
        // User templates first (by name), then built-ins not overridden.
        assert_eq!(&ids[..2], ["review-file", "zeta"]);
        assert_eq!(lib.templates[0].name, "My review");
        assert!(!lib.templates[0].is_builtin());
        assert_eq!(ids.iter().filter(|i| **i == "review-file").count(), 1);
        assert_eq!(lib.templates.len(), 2 + BUILTIN.len() - 1);
        assert_eq!(lib.problems, ["bad.md: not UTF-8 text, skipped"]);

        // A folder that does not exist is simply "no user templates".
        let lib = load(Some(&dir.join("missing")));
        assert_eq!(lib.templates.len(), BUILTIN.len());
        assert!(lib.problems.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn user_dir_is_next_to_the_settings_file() {
        let dir = user_dir(Path::new("/cfg/forgeterm/settings.toml")).unwrap();
        assert_eq!(dir, Path::new("/cfg/forgeterm/prompts"));
    }
}
