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
    /// A hook in `.claude/settings*.json`: a command Claude Code runs on an event.
    Hook,
    Mcp,
    /// One server from `.mcp.json`.
    McpServer,
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
            ContextKind::Hook => "Hook",
            ContextKind::Mcp => "MCP",
            ContextKind::McpServer => "MCP server",
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
        let git = dir.join(".git");
        if forgeterm_fs::resolves_locally(&git) && git.exists() {
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
        if local_file(&path) {
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
        if local_file(&file) {
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
    // What the settings and .mcp.json files declare (shown, never run).
    let mut declared = Vec::new();
    for item in &items {
        match item.kind {
            ContextKind::Settings => declared.extend(hooks_in(&item.path)),
            ContextKind::Mcp => declared.extend(mcp_servers_in(&item.path)),
            _ => {}
        }
    }
    items.extend(declared);
    items.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));
    ProjectContext {
        root,
        is_git,
        items,
    }
}

/// Largest settings / `.mcp.json` file parsed.
const MAX_JSON_BYTES: u64 = 512 * 1024;
/// Longest command text shown per item.
const MAX_SHOWN: usize = 200;

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let meta = fs::metadata(path).ok()?;
    if meta.len() > MAX_JSON_BYTES {
        return None;
    }
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

fn shorten(s: &str) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() > MAX_SHOWN {
        let cut: String = one_line.chars().take(MAX_SHOWN).collect();
        format!("{cut}…")
    } else {
        one_line
    }
}

/// Hooks as documented for Claude Code settings:
/// `{"hooks": {"<Event>": [{"matcher": "…", "hooks": [{"type": "command", "command": "…"}]}]}}`.
fn hooks_in(path: &Path) -> Vec<ContextItem> {
    let Some(json) = read_json(path) else {
        return Vec::new();
    };
    parse_hooks(&json, path)
}

fn parse_hooks(json: &serde_json::Value, path: &Path) -> Vec<ContextItem> {
    let Some(events) = json.get("hooks").and_then(|h| h.as_object()) else {
        return Vec::new();
    };
    let file = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut out = Vec::new();
    for (event, groups) in events {
        for group in groups.as_array().into_iter().flatten() {
            let matcher = group
                .get("matcher")
                .and_then(|m| m.as_str())
                .filter(|m| !m.is_empty());
            let commands: Vec<String> = group
                .get("hooks")
                .and_then(|h| h.as_array())
                .into_iter()
                .flatten()
                .filter_map(|h| {
                    let kind = h.get("type").and_then(|t| t.as_str()).unwrap_or("command");
                    match kind {
                        "command" => h.get("command").and_then(|c| c.as_str()).map(str::to_owned),
                        other => Some(format!("({other} hook)")),
                    }
                })
                .collect();
            if commands.is_empty() {
                continue;
            }
            out.push(ContextItem {
                kind: ContextKind::Hook,
                name: match matcher {
                    Some(m) => format!("{event} · {m}"),
                    None => event.clone(),
                },
                description: Some(format!("{} ({file})", shorten(&commands.join("; ")))),
                path: path.to_path_buf(),
            });
        }
    }
    out
}

/// Servers as documented for `.mcp.json`:
/// `{"mcpServers": {"<name>": {"command": "…", "args": […]} | {"type": "http", "url": "…"}}}`.
fn mcp_servers_in(path: &Path) -> Vec<ContextItem> {
    let Some(json) = read_json(path) else {
        return Vec::new();
    };
    parse_mcp_servers(&json, path)
}

fn parse_mcp_servers(json: &serde_json::Value, path: &Path) -> Vec<ContextItem> {
    let Some(servers) = json.get("mcpServers").and_then(|s| s.as_object()) else {
        return Vec::new();
    };
    servers
        .iter()
        .map(|(name, server)| {
            let text = |key: &str| server.get(key).and_then(|v| v.as_str());
            let description = if let Some(url) = text("url") {
                format!("{}: {url}", text("type").unwrap_or("remote"))
            } else if let Some(command) = text("command") {
                let args: Vec<&str> = server
                    .get("args")
                    .and_then(|a| a.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|a| a.as_str())
                    .collect();
                format!(
                    "runs: {}",
                    shorten(
                        &[command]
                            .into_iter()
                            .chain(args)
                            .collect::<Vec<_>>()
                            .join(" ")
                    )
                )
            } else {
                "no command or url".to_owned()
            };
            ContextItem {
                kind: ContextKind::McpServer,
                name: name.clone(),
                description: Some(description),
                path: path.to_path_buf(),
            }
        })
        .collect()
}

fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    if !forgeterm_fs::resolves_locally(dir) {
        return Vec::new();
    }
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
        if forgeterm_fs::resolves_locally(&p) && p.is_dir() {
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

/// An existing regular file, reached without leaving this machine (a link in the
/// project could point to a network share).
fn local_file(p: &Path) -> bool {
    forgeterm_fs::resolves_locally(p) && p.is_file()
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
    fn hooks_and_mcp_servers_are_listed_not_run() {
        let settings: serde_json::Value = serde_json::from_str(
            r#"{
              "permissions": {"allow": ["Bash(cargo test:*)"]},
              "hooks": {
                "PreToolUse": [
                  {"matcher": "Bash", "hooks": [{"type": "command", "command": "./check.sh"}, {"type": "command", "command": "echo  two\n lines"}]},
                  {"matcher": "", "hooks": []}
                ],
                "Stop": [{"hooks": [{"type": "prompt", "prompt": "x"}]}]
              }
            }"#,
        )
        .unwrap();
        let path = Path::new("/p/.claude/settings.json");
        let hooks = parse_hooks(&settings, path);
        assert_eq!(hooks.len(), 2, "{hooks:?}");
        assert_eq!(hooks[0].name, "PreToolUse · Bash");
        assert_eq!(
            hooks[0].description.as_deref(),
            Some("./check.sh; echo two lines (settings.json)")
        );
        assert_eq!(hooks[1].name, "Stop");
        assert_eq!(
            hooks[1].description.as_deref(),
            Some("(prompt hook) (settings.json)")
        );
        assert!(parse_hooks(&serde_json::json!({"hooks": 3}), path).is_empty());

        let mcp: serde_json::Value = serde_json::from_str(
            r#"{"mcpServers": {
                "files": {"command": "npx", "args": ["-y", "@scope/server", "."]},
                "docs": {"type": "http", "url": "https://example.com/mcp"},
                "odd": {}
            }}"#,
        )
        .unwrap();
        let servers = parse_mcp_servers(&mcp, Path::new("/p/.mcp.json"));
        let shown: Vec<(&str, &str)> = servers
            .iter()
            .map(|s| (s.name.as_str(), s.description.as_deref().unwrap()))
            .collect();
        assert_eq!(
            shown,
            [
                ("docs", "http: https://example.com/mcp"),
                ("files", "runs: npx -y @scope/server ."),
                ("odd", "no command or url"),
            ]
        );
        let long = "x".repeat(500);
        assert!(shorten(&long).ends_with('…'));
        assert_eq!(shorten(&long).chars().count(), MAX_SHOWN + 1);
    }

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
