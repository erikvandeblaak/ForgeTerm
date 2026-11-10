//! Project context: the files the agents themselves read in a project. Read-only.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContextKind {
    /// CLAUDE.md and variants (Claude Code project memory).
    Memory,
    /// AGENTS.md (Codex project instructions).
    AgentsMd,
    Skill,
    Command,
    Subagent,
    Settings,
    Mcp,
}

impl ContextKind {
    pub fn label(self) -> &'static str {
        match self {
            ContextKind::Memory => "Memory",
            ContextKind::AgentsMd => "Instructions",
            ContextKind::Skill => "Skill",
            ContextKind::Command => "Command",
            ContextKind::Subagent => "Subagent",
            ContextKind::Settings => "Settings",
            ContextKind::Mcp => "MCP",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextItem {
    pub kind: ContextKind,
    /// Display name: file name, skill/command/subagent name.
    pub name: String,
    pub path: PathBuf,
    /// `description:` from the front matter, if any.
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectContext {
    pub root: PathBuf,
    /// The root was found through a `.git` entry (otherwise it is the start folder).
    pub is_git: bool,
    pub items: Vec<ContextItem>,
}

/// The project a folder belongs to: the nearest ancestor (or itself) containing
/// `.git` (a folder, or a file for worktrees/submodules). Without one, `start`.
pub fn project_root(start: &Path) -> (PathBuf, bool) {
    for dir in start.ancestors() {
        if dir.join(".git").exists() {
            return (dir.to_path_buf(), true);
        }
    }
    (start.to_path_buf(), false)
}

/// Lists agent files in the project containing `start`.
pub fn scan_context(start: &Path) -> ProjectContext {
    let (root, is_git) = project_root(start);
    let mut items = Vec::new();
    let claude = root.join(".claude");

    for (rel, kind) in [
        ("CLAUDE.md", ContextKind::Memory),
        (".claude/CLAUDE.md", ContextKind::Memory),
        ("CLAUDE.local.md", ContextKind::Memory),
        ("AGENTS.md", ContextKind::AgentsMd),
        (".claude/settings.json", ContextKind::Settings),
        (".claude/settings.local.json", ContextKind::Settings),
        (".mcp.json", ContextKind::Mcp),
    ] {
        let path = root.join(rel);
        if path.is_file() {
            items.push(ContextItem {
                kind,
                name: rel.to_owned(),
                path,
                description: None,
            });
        }
    }

    // .claude/rules/*.md (loaded like CLAUDE.md)
    let mut rules = Vec::new();
    collect_md(&claude.join("rules"), &mut rules, 0);
    for file in rules {
        let rel = file
            .strip_prefix(&root)
            .map_or_else(|_| file.clone(), Path::to_path_buf);
        items.push(ContextItem {
            kind: ContextKind::Memory,
            name: rel.to_string_lossy().replace('\\', "/"),
            description: None,
            path: file,
        });
    }

    // .claude/skills/<name>/SKILL.md
    for dir in sorted_entries(&claude.join("skills")) {
        let file = dir.join("SKILL.md");
        if file.is_file() {
            let meta = front_matter(&file);
            items.push(ContextItem {
                kind: ContextKind::Skill,
                name: meta.name.unwrap_or_else(|| file_stem(&dir)),
                description: meta.description,
                path: file,
            });
        }
    }
    // .claude/commands/**/*.md and .claude/agents/*.md
    for (sub, kind) in [
        ("commands", ContextKind::Command),
        ("agents", ContextKind::Subagent),
    ] {
        let mut files = Vec::new();
        collect_md(&claude.join(sub), &mut files, 0);
        for file in files {
            let meta = front_matter(&file);
            let name = match kind {
                ContextKind::Command => format!("/{}", file_stem(&file)),
                _ => meta.name.clone().unwrap_or_else(|| file_stem(&file)),
            };
            items.push(ContextItem {
                kind,
                name,
                description: meta.description,
                path: file,
            });
        }
    }
    items.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));
    ProjectContext {
        root,
        is_git,
        items,
    }
}

fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// Markdown files below `dir` (commands may be grouped in sub-folders). Depth-limited.
fn collect_md(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 3 {
        return;
    }
    for p in sorted_entries(dir) {
        if p.is_dir() {
            collect_md(&p, out, depth + 1);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")) {
            out.push(p);
        }
    }
}

fn file_stem(p: &Path) -> String {
    p.file_stem()
        .or_else(|| p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[derive(Debug, Default, PartialEq, Eq)]
struct FrontMatter {
    name: Option<String>,
    description: Option<String>,
}

/// Reads `name:` / `description:` from a leading `---` YAML block (first 8 KB only).
fn front_matter(path: &Path) -> FrontMatter {
    let mut buf = Vec::new();
    if let Ok(f) = fs::File::open(path) {
        let _ = f.take(8192).read_to_end(&mut buf);
    }
    parse_front_matter(&String::from_utf8_lossy(&buf))
}

fn parse_front_matter(text: &str) -> FrontMatter {
    let mut fm = FrontMatter::default();
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return fm;
    }
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.starts_with(char::is_whitespace) {
            continue; // nested YAML
        }
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'').trim();
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            "name" => fm.name = Some(value.to_owned()),
            "description" => fm.description = Some(value.to_owned()),
            _ => {}
        }
    }
    fm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_fields() {
        let fm = parse_front_matter(
            "---\nname: review\ndescription: \"Review the diff\"\nallowed-tools: Bash\n  nested: x\n---\n# Body\ndescription: not this",
        );
        assert_eq!(fm.name.as_deref(), Some("review"));
        assert_eq!(fm.description.as_deref(), Some("Review the diff"));
        assert_eq!(
            parse_front_matter("# no front matter\nname: x"),
            FrontMatter::default()
        );
    }

    #[test]
    fn scans_a_project_tree() {
        let root = std::env::temp_dir().join(format!("forgeterm-ctx-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let sub = root.join("src").join("deep");
        fs::create_dir_all(&sub).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join(".claude/skills/release")).unwrap();
        fs::create_dir_all(root.join(".claude/commands/git")).unwrap();
        fs::create_dir_all(root.join(".claude/agents")).unwrap();
        fs::write(root.join("CLAUDE.md"), "# rules").unwrap();
        fs::create_dir_all(root.join(".claude/rules")).unwrap();
        fs::write(root.join(".claude/rules/style.md"), "Use tabs").unwrap();
        fs::write(root.join("AGENTS.md"), "# codex").unwrap();
        fs::write(root.join(".mcp.json"), "{}").unwrap();
        fs::write(root.join(".claude/settings.json"), "{}").unwrap();
        fs::write(
            root.join(".claude/skills/release/SKILL.md"),
            "---\nname: release\ndescription: Cut a release\n---\n",
        )
        .unwrap();
        fs::write(root.join(".claude/commands/git/sync.md"), "Sync the branch").unwrap();
        fs::write(
            root.join(".claude/agents/reviewer.md"),
            "---\nname: code-reviewer\ndescription: Reviews code\n---\n",
        )
        .unwrap();

        // Started from a nested folder: the git root is found.
        let ctx = scan_context(&sub);
        assert_eq!(ctx.root, root);
        assert!(ctx.is_git);
        let got: Vec<(ContextKind, &str, Option<&str>)> = ctx
            .items
            .iter()
            .map(|i| (i.kind, i.name.as_str(), i.description.as_deref()))
            .collect();
        assert_eq!(
            got,
            vec![
                (ContextKind::Memory, ".claude/rules/style.md", None),
                (ContextKind::Memory, "CLAUDE.md", None),
                (ContextKind::AgentsMd, "AGENTS.md", None),
                (ContextKind::Skill, "release", Some("Cut a release")),
                (ContextKind::Command, "/sync", None),
                (ContextKind::Subagent, "code-reviewer", Some("Reviews code")),
                (ContextKind::Settings, ".claude/settings.json", None),
                (ContextKind::Mcp, ".mcp.json", None),
            ]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn without_git_the_start_folder_is_the_root() {
        let dir = std::env::temp_dir().join(format!("forgeterm-nogit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // The temp dir itself is (normally) not inside a git repository.
        let (root, is_git) = project_root(&dir);
        if !is_git {
            assert_eq!(root, dir);
        }
        assert!(scan_context(&dir).items.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
