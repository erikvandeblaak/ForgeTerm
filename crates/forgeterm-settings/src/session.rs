//! The last session's layout (`session.toml` next to `settings.toml`): tabs, their
//! splits with ratios, each pane's shell and folder, the active tab and the files
//! open in the viewer. Never terminal output, commands or environment.
//!
//! Format: one `[[tab]]` per tab with its layout as a flat list in prefix order
//! (`{ split = "right" | "down", ratio }` followed by its two parts, or a pane
//! `{ shell, cwd }`), so the file stays readable. Reading is tolerant: anything
//! malformed is dropped and the rest is used.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

pub const FILE_NAME: &str = "session.toml";
/// Limits against a corrupted or hand-edited file.
pub const MAX_TABS: usize = 32;
pub const MAX_PANES_PER_TAB: usize = 16;
pub const MAX_VIEWER_FILES: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub enum Layout {
    Pane(PaneSpec),
    Split {
        /// `true` = side by side ("right"), `false` = stacked ("down").
        side_by_side: bool,
        ratio: f32,
        a: Box<Layout>,
        b: Box<Layout>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneSpec {
    /// Shell program as started (`C:\...\pwsh.exe`, `/bin/bash`).
    pub shell: String,
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SavedSession {
    pub tabs: Vec<Layout>,
    pub active: usize,
    pub viewer_files: Vec<PathBuf>,
}

/// `session.toml` in the folder of the settings file.
pub fn path_for(settings_file: &Path) -> Option<PathBuf> {
    settings_file.parent().map(|d| d.join(FILE_NAME))
}

pub fn to_toml(session: &SavedSession) -> String {
    let mut root = Table::new();
    root.insert("active".into(), Value::Integer(session.active as i64));
    if !session.viewer_files.is_empty() {
        root.insert(
            "viewer_files".into(),
            Value::Array(
                session
                    .viewer_files
                    .iter()
                    .map(|p| Value::String(p.to_string_lossy().into_owned()))
                    .collect(),
            ),
        );
    }
    let tabs = session
        .tabs
        .iter()
        .map(|layout| {
            let mut items = Vec::new();
            flatten(layout, &mut items);
            let mut t = Table::new();
            t.insert("layout".into(), Value::Array(items));
            Value::Table(t)
        })
        .collect();
    root.insert("tab".into(), Value::Array(tabs));
    format!(
        "# ForgeTerm session: written automatically, read at startup when\n\
         # [workspace] restore_session = true. Safe to delete.\n\n{}",
        toml::to_string(&root).unwrap_or_default()
    )
}

fn flatten(layout: &Layout, out: &mut Vec<Value>) {
    let mut t = Table::new();
    match layout {
        Layout::Pane(p) => {
            t.insert("shell".into(), Value::String(p.shell.clone()));
            if let Some(cwd) = &p.cwd {
                t.insert(
                    "cwd".into(),
                    Value::String(cwd.to_string_lossy().into_owned()),
                );
            }
            out.push(Value::Table(t));
        }
        Layout::Split {
            side_by_side,
            ratio,
            a,
            b,
        } => {
            t.insert(
                "split".into(),
                Value::String(if *side_by_side { "right" } else { "down" }.into()),
            );
            t.insert("ratio".into(), Value::Float(f64::from(*ratio)));
            out.push(Value::Table(t));
            flatten(a, out);
            flatten(b, out);
        }
    }
}

/// Parses a session file; malformed tabs are skipped.
pub fn parse(text: &str) -> SavedSession {
    let Ok(root) = text.parse::<Table>() else {
        return SavedSession::default();
    };
    let mut tabs = Vec::new();
    if let Some(Value::Array(list)) = root.get("tab") {
        for tab in list.iter().take(MAX_TABS) {
            let Some(Value::Array(items)) = tab.as_table().and_then(|t| t.get("layout")) else {
                continue;
            };
            let mut iter = items.iter();
            let mut panes = 0;
            if let Some(layout) = unflatten(&mut iter, &mut panes, 0)
                && iter.next().is_none()
            {
                tabs.push(layout);
            }
        }
    }
    let active = root
        .get("active")
        .and_then(Value::as_integer)
        .and_then(|i| usize::try_from(i).ok())
        .unwrap_or(0)
        .min(tabs.len().saturating_sub(1));
    let viewer_files = match root.get("viewer_files") {
        Some(Value::Array(list)) => list
            .iter()
            .filter_map(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .take(MAX_VIEWER_FILES)
            .map(PathBuf::from)
            .collect(),
        _ => Vec::new(),
    };
    SavedSession {
        tabs,
        active,
        viewer_files,
    }
}

fn unflatten<'a>(
    items: &mut impl Iterator<Item = &'a Value>,
    panes: &mut usize,
    depth: usize,
) -> Option<Layout> {
    if depth > MAX_PANES_PER_TAB {
        return None;
    }
    let t = items.next()?.as_table()?;
    if let Some(split) = t.get("split").and_then(Value::as_str) {
        let side_by_side = match split {
            "right" => true,
            "down" => false,
            _ => return None,
        };
        let ratio = match t.get("ratio") {
            Some(Value::Float(f)) if f.is_finite() => *f as f32,
            Some(Value::Integer(i)) => *i as f32,
            _ => 0.5,
        }
        .clamp(0.1, 0.9);
        let a = unflatten(items, panes, depth + 1)?;
        let b = unflatten(items, panes, depth + 1)?;
        return Some(Layout::Split {
            side_by_side,
            ratio,
            a: Box::new(a),
            b: Box::new(b),
        });
    }
    let shell = t.get("shell").and_then(Value::as_str)?.trim();
    if shell.is_empty() {
        return None;
    }
    *panes += 1;
    if *panes > MAX_PANES_PER_TAB {
        return None;
    }
    let cwd = t
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from);
    Some(Layout::Pane(PaneSpec {
        shell: shell.to_owned(),
        cwd,
    }))
}

pub fn load(path: &Path) -> SavedSession {
    fs::read_to_string(path)
        .map(|t| parse(&t))
        .unwrap_or_default()
}

/// Writes atomically (temporary file, then rename), so a crash never leaves a
/// half-written session.
pub fn save(path: &Path, session: &SavedSession) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, to_toml(session))?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(shell: &str, cwd: Option<&str>) -> Layout {
        Layout::Pane(PaneSpec {
            shell: shell.into(),
            cwd: cwd.map(PathBuf::from),
        })
    }

    fn sample() -> SavedSession {
        SavedSession {
            tabs: vec![
                pane(
                    r"C:\Program Files\PowerShell\7\pwsh.exe",
                    Some(r"C:\Projects\a"),
                ),
                Layout::Split {
                    side_by_side: true,
                    ratio: 0.25,
                    a: Box::new(pane("cmd.exe", None)),
                    b: Box::new(Layout::Split {
                        side_by_side: false,
                        ratio: 0.5,
                        a: Box::new(pane("/bin/bash", Some("/home/x/my 'repo'"))),
                        b: Box::new(pane("cmd.exe", Some("D:\\"))),
                    }),
                },
            ],
            active: 1,
            viewer_files: vec![PathBuf::from(r"C:\Projects\a\src\main.rs")],
        }
    }

    #[test]
    fn round_trips_tabs_splits_and_files() {
        let s = sample();
        let text = to_toml(&s);
        assert!(text.starts_with("# ForgeTerm session"));
        assert_eq!(parse(&text), s);
    }

    #[test]
    fn tolerates_damage() {
        assert_eq!(parse("not toml = = ="), SavedSession::default());
        assert_eq!(parse(""), SavedSession::default());
        // A split missing its second part, an unknown split kind and an empty shell
        // are dropped; the valid tab survives. `active` is clamped.
        let text = r#"
            active = 9
            [[tab]]
            layout = [{ split = "right", ratio = 0.5 }, { shell = "cmd.exe" }]
            [[tab]]
            layout = [{ split = "diagonal" }, { shell = "a" }, { shell = "b" }]
            [[tab]]
            layout = [{ shell = "  " }]
            [[tab]]
            layout = [{ shell = "pwsh.exe", cwd = "" }, { shell = "extra" }]
            [[tab]]
            layout = [{ split = "down", ratio = 7 }, { shell = "a" }, { shell = "b" }]
        "#;
        let s = parse(text);
        assert_eq!(s.tabs.len(), 1, "{s:?}");
        match &s.tabs[0] {
            Layout::Split { ratio, .. } => assert_eq!(*ratio, 0.9, "ratio clamped"),
            other => panic!("{other:?}"),
        }
        assert_eq!(s.active, 0);
    }

    #[test]
    fn limits_panes_per_tab() {
        // A chain of splits with more panes than allowed is refused.
        let mut items = Vec::new();
        for _ in 0..MAX_PANES_PER_TAB {
            items.push(r#"{ split = "right", ratio = 0.5 }, { shell = "a" }"#.to_owned());
        }
        items.push(r#"{ shell = "last" }"#.to_owned());
        let text = format!("[[tab]]\nlayout = [{}]\n", items.join(", "));
        assert!(parse(&text).tabs.is_empty());
    }

    #[test]
    fn saves_atomically_and_loads() {
        let dir = std::env::temp_dir().join(format!("forgeterm-session-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join(FILE_NAME);
        assert_eq!(load(&path), SavedSession::default(), "missing = empty");
        save(&path, &sample()).unwrap();
        assert_eq!(load(&path), sample());
        assert!(!path.with_extension("toml.tmp").exists());
        assert_eq!(
            path_for(&dir.join("settings.toml")).unwrap(),
            dir.join(FILE_NAME)
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
