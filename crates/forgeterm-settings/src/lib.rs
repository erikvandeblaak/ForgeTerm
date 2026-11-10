//! ForgeTerm settings (`settings.toml`).
//!
//! Loading never fails: a missing file means defaults, and every problem (syntax
//! error, unknown key, wrong type, out-of-range value) is reported in
//! [`Loaded::problems`] while the affected setting keeps its default. ForgeTerm never
//! writes to an existing settings file; [`ensure_file`] only creates a missing one.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

pub const FILE_NAME: &str = "settings.toml";

pub const MIN_FONT_SIZE: f32 = 8.0;
pub const MAX_FONT_SIZE: f32 = 32.0;
pub const MAX_SCROLLBACK: i64 = 100_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Terminal {
    pub font_size: f32,
    pub scrollback: usize,
    /// Shell name as listed in the "New tab with…" menu; `None` = first detected.
    pub default_shell: Option<String>,
    /// Directory for the first tab (and when a shell's directory is unknown);
    /// `None` = where ForgeTerm was started.
    pub start_directory: Option<PathBuf>,
    pub shell_integration: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explorer {
    pub visible: bool,
    pub follow_terminal: bool,
    pub show_hidden: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub terminal: Terminal,
    pub explorer: Explorer,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            terminal: Terminal {
                font_size: 14.0,
                scrollback: 10_000,
                default_shell: None,
                start_directory: None,
                shell_integration: true,
            },
            explorer: Explorer {
                visible: false,
                follow_terminal: true,
                show_hidden: false,
            },
        }
    }
}

/// Result of loading: always usable settings plus human-readable problems.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub settings: Settings,
    pub problems: Vec<String>,
    /// The file could not be read or parsed at all; `settings` are plain defaults.
    /// A running app should keep its current settings instead of applying them.
    pub unreadable: bool,
}

/// Written by [`ensure_file`]; parses to exactly the defaults.
pub const TEMPLATE: &str = r#"# ForgeTerm settings
#
# Changes are applied automatically when you save this file.
# Remove a line (or comment it out with #) to use the default.

[terminal]
# Font size in points (8 to 32). Ctrl+= / Ctrl+- change it for the current session.
font_size = 14

# Lines kept per terminal for scrolling back (0 to 100000). Applies to new terminals.
scrollback = 10000

# Shell for new tabs, by its name in the "New tab with…" menu, for example
# "PowerShell 7", "Command Prompt" or "Git Bash". Empty = first detected shell.
default_shell = ""

# Folder the first tab starts in. Empty = the folder ForgeTerm was started from.
start_directory = ""

# Let shells report their current folder, so new tabs and the explorer follow it.
# Uses the session's arguments/environment only; your profile files are not changed.
shell_integration = true

[explorer]
# Show the explorer side panel at startup (Ctrl+Shift+E toggles it).
visible = false

# The explorer follows the folder of the focused terminal.
follow_terminal = true

# Show dot-files and files marked hidden.
show_hidden = false
"#;

/// Where the settings file lives: `%APPDATA%\ForgeTerm` on Windows,
/// `$XDG_CONFIG_HOME/forgeterm` or `~/.config/forgeterm` elsewhere.
/// `FORGETERM_SETTINGS` overrides the location (tests, portable setups).
pub fn default_path() -> Option<PathBuf> {
    if let Some(custom) = std::env::var_os("FORGETERM_SETTINGS").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(custom));
    }
    let dir = if cfg!(windows) {
        PathBuf::from(std::env::var_os("APPDATA")?).join("ForgeTerm")
    } else if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        PathBuf::from(xdg).join("forgeterm")
    } else {
        PathBuf::from(std::env::var_os("HOME")?)
            .join(".config")
            .join("forgeterm")
    };
    Some(dir.join(FILE_NAME))
}

/// Loads `path`; a missing file gives defaults without problems.
pub fn load(path: &Path) -> Loaded {
    match fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Loaded {
            settings: Settings::default(),
            problems: Vec::new(),
            unreadable: false,
        },
        Err(e) => Loaded {
            settings: Settings::default(),
            problems: vec![format!("could not read {}: {e}", path.display())],
            unreadable: true,
        },
    }
}

/// Creates the file with [`TEMPLATE`] if it does not exist. Never overwrites.
pub fn ensure_file(path: &Path) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut f) => io::Write::write_all(&mut f, TEMPLATE.as_bytes()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

/// Parses settings text leniently.
pub fn parse(text: &str) -> Loaded {
    let mut settings = Settings::default();
    let mut problems = Vec::new();
    let table: Table = match text.parse() {
        Ok(t) => t,
        Err(e) => {
            problems.push(format!("syntax error: {}", one_line(&e.to_string())));
            return Loaded {
                settings,
                problems,
                unreadable: true,
            };
        }
    };

    for (key, value) in &table {
        match (key.as_str(), value) {
            ("terminal", Value::Table(t)) => {
                parse_terminal(t, &mut settings.terminal, &mut problems)
            }
            ("explorer", Value::Table(t)) => {
                parse_explorer(t, &mut settings.explorer, &mut problems)
            }
            ("terminal" | "explorer", _) => problems.push(format!("[{key}] must be a section")),
            _ => problems.push(format!("unknown setting '{key}' (ignored)")),
        }
    }
    Loaded {
        settings,
        problems,
        unreadable: false,
    }
}

fn parse_terminal(t: &Table, out: &mut Terminal, problems: &mut Vec<String>) {
    for (key, value) in t {
        let name = format!("terminal.{key}");
        match key.as_str() {
            "font_size" => {
                if let Some(v) = number(&name, value, problems) {
                    let clamped = v.clamp(f64::from(MIN_FONT_SIZE), f64::from(MAX_FONT_SIZE));
                    if clamped != v {
                        problems.push(format!(
                            "{name} = {v} is outside {MIN_FONT_SIZE}–{MAX_FONT_SIZE}; using {clamped}"
                        ));
                    }
                    out.font_size = clamped as f32;
                }
            }
            "scrollback" => match value {
                Value::Integer(v) if (0..=MAX_SCROLLBACK).contains(v) => {
                    out.scrollback = *v as usize
                }
                Value::Integer(v) => {
                    let clamped = (*v).clamp(0, MAX_SCROLLBACK);
                    problems.push(format!(
                        "{name} = {v} is outside 0–{MAX_SCROLLBACK}; using {clamped}"
                    ));
                    out.scrollback = clamped as usize;
                }
                other => problems.push(wrong_type(&name, "a whole number", other)),
            },
            "default_shell" => {
                if let Some(s) = string(&name, value, problems) {
                    out.default_shell = non_empty(s);
                }
            }
            "start_directory" => {
                if let Some(s) = string(&name, value, problems) {
                    out.start_directory = non_empty(s).map(PathBuf::from);
                }
            }
            "shell_integration" => {
                if let Some(b) = boolean(&name, value, problems) {
                    out.shell_integration = b;
                }
            }
            _ => problems.push(format!("unknown setting '{name}' (ignored)")),
        }
    }
}

fn parse_explorer(t: &Table, out: &mut Explorer, problems: &mut Vec<String>) {
    for (key, value) in t {
        let name = format!("explorer.{key}");
        let target = match key.as_str() {
            "visible" => &mut out.visible,
            "follow_terminal" => &mut out.follow_terminal,
            "show_hidden" => &mut out.show_hidden,
            _ => {
                problems.push(format!("unknown setting '{name}' (ignored)"));
                continue;
            }
        };
        if let Some(b) = boolean(&name, value, problems) {
            *target = b;
        }
    }
}

fn number(name: &str, value: &Value, problems: &mut Vec<String>) -> Option<f64> {
    match value {
        Value::Integer(i) => Some(*i as f64),
        Value::Float(f) if f.is_finite() => Some(*f),
        other => {
            problems.push(wrong_type(name, "a number", other));
            None
        }
    }
}

fn string<'a>(name: &str, value: &'a Value, problems: &mut Vec<String>) -> Option<&'a str> {
    match value {
        Value::String(s) => Some(s),
        other => {
            problems.push(wrong_type(name, "text in quotes", other));
            None
        }
    }
}

fn boolean(name: &str, value: &Value, problems: &mut Vec<String>) -> Option<bool> {
    match value {
        Value::Boolean(b) => Some(*b),
        other => {
            problems.push(wrong_type(name, "true or false", other));
            None
        }
    }
}

fn wrong_type(name: &str, expected: &str, got: &Value) -> String {
    format!(
        "{name} must be {expected} (found {}); using the default",
        got.type_str()
    )
}

fn non_empty(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_owned())
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_is_exactly_the_defaults() {
        let loaded = parse(TEMPLATE);
        assert_eq!(loaded.problems, Vec::<String>::new());
        assert_eq!(loaded.settings, Settings::default());
    }

    #[test]
    fn empty_file_is_defaults() {
        assert_eq!(
            parse(""),
            Loaded {
                settings: Settings::default(),
                problems: vec![],
                unreadable: false,
            }
        );
    }

    #[test]
    fn values_are_applied() {
        let s = parse(
            r#"
            [terminal]
            font_size = 16.5
            scrollback = 0
            default_shell = " Git Bash "
            start_directory = 'C:\Projects'
            shell_integration = false
            [explorer]
            visible = true
            follow_terminal = false
            show_hidden = true
            "#,
        );
        assert!(s.problems.is_empty(), "{:?}", s.problems);
        let t = &s.settings.terminal;
        assert_eq!(t.font_size, 16.5);
        assert_eq!(t.scrollback, 0);
        assert_eq!(t.default_shell.as_deref(), Some("Git Bash"));
        assert_eq!(t.start_directory, Some(PathBuf::from(r"C:\Projects")));
        assert!(!t.shell_integration);
        assert_eq!(
            s.settings.explorer,
            Explorer {
                visible: true,
                follow_terminal: false,
                show_hidden: true
            }
        );
    }

    #[test]
    fn bad_values_fall_back_with_a_problem_each() {
        let s = parse(
            r#"
            colour = "red"
            [terminal]
            font_size = 200
            scrollback = 5000000
            shell_integration = "yes"
            default_shell = 3
            fontsize = 12
            [explorer]
            visible = 1
            "#,
        );
        let t = &s.settings.terminal;
        assert_eq!(t.font_size, MAX_FONT_SIZE);
        assert_eq!(t.scrollback, MAX_SCROLLBACK as usize);
        assert!(t.shell_integration, "wrong type keeps the default");
        assert_eq!(t.default_shell, None);
        assert!(!s.settings.explorer.visible);
        assert_eq!(s.problems.len(), 7, "{:#?}", s.problems);
        let all = s.problems.join("\n");
        for needle in [
            "unknown setting 'colour'",
            "terminal.font_size = 200",
            "terminal.scrollback = 5000000",
            "terminal.shell_integration must be true or false",
            "terminal.default_shell must be text",
            "unknown setting 'terminal.fontsize'",
            "explorer.visible must be true or false",
        ] {
            assert!(all.contains(needle), "missing {needle:?} in\n{all}");
        }
    }

    #[test]
    fn syntax_error_keeps_all_defaults() {
        let s = parse("[terminal]\nfont_size = = 3\n");
        assert!(s.unreadable);
        assert_eq!(s.settings, Settings::default());
        assert_eq!(s.problems.len(), 1);
        assert!(
            s.problems[0].starts_with("syntax error"),
            "{}",
            s.problems[0]
        );
        assert!(!s.problems[0].contains('\n'));
    }

    #[test]
    fn section_must_be_a_table() {
        let s = parse("terminal = 5\n");
        assert_eq!(s.problems, vec!["[terminal] must be a section".to_owned()]);
    }

    #[test]
    fn ensure_file_creates_once_and_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("forgeterm-settings-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("nested").join(FILE_NAME);
        assert_eq!(
            load(&path).problems,
            Vec::<String>::new(),
            "missing file = defaults"
        );

        ensure_file(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), TEMPLATE);

        fs::write(&path, "[terminal]\nfont_size = 20\n").unwrap();
        ensure_file(&path).unwrap();
        assert_eq!(
            load(&path).settings.terminal.font_size,
            20.0,
            "user file kept"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
