//! Background work and OS helpers (threads, files, URLs).

use super::*;

/// The folder ForgeTerm was started from.
pub(super) fn launch_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub(super) fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Opens a text file for editing (detached). Windows: Notepad, which is always
/// present (`.toml` usually has no file association, which would only show the
/// "How do you want to open this file?" dialog). Elsewhere: the desktop's default.
pub(super) fn open_in_editor(path: &Path) -> Result<(), String> {
    let program = if cfg!(windows) {
        "notepad.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(path)
        .spawn()
        .map(drop)
        .map_err(|e| format!("could not open {} with {program}: {e}", path.display()))
}

/// Lists a directory on a background thread (slow or network drives must never
/// block the UI) and reports the result.
/// Runs `f` on a background thread and maps its result to a message.
pub(super) fn background<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
    to_message: impl Fn(T) -> Message + Send + 'static,
) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(f());
            });
            rx.await
        },
        move |result| match result {
            Ok(v) => to_message(v),
            // The worker thread panicked: reset the busy flags (and log it).
            Err(_) => Message::WorkerPanicked,
        },
    )
}

/// Locates the agents and asks each for its version (off the UI thread; a slow
/// virus scan of the executable must not freeze the window).
pub(super) fn detect_agents() -> Task<Message> {
    background(
        || {
            AgentKind::ALL
                .iter()
                .map(|&kind| {
                    let agent = forgeterm_agents::locate(kind).map(|path| Agent {
                        kind,
                        version: forgeterm_agents::query_version(
                            &path,
                            std::time::Duration::from_secs(20),
                        ),
                        path,
                    });
                    (kind, agent)
                })
                .collect()
        },
        Message::AgentsDetected,
    )
}

/// Opens a web link from terminal output in the default browser, only after a
/// Ctrl+click and only for http(s) links that pass `is_openable` (terminal output
/// is untrusted). The link is passed as one argument, never through a shell.
pub(super) fn open_url(url: &str) -> Result<(), String> {
    if !forgeterm_terminal::is_openable(url) {
        return Err(format!("not opened: {url} is not a plain http(s) link"));
    }
    let program = if cfg!(windows) {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(url)
        .spawn()
        .map(drop)
        .map_err(|e| format!("could not open the link with {program}: {e}"))
}

/// Writes a rendered frame as PNG (test hook, see FORGETERM_SCREENSHOT).
pub(super) fn save_screenshot(shot: &iced::window::Screenshot, path: &Path) {
    let size = resvg::tiny_skia::IntSize::from_wh(shot.size.width, shot.size.height);
    let png = size
        .and_then(|s| resvg::tiny_skia::Pixmap::from_vec(shot.rgba.to_vec(), s))
        .and_then(|p| p.encode_png().ok());
    match png.map(|bytes| std::fs::write(path, bytes)) {
        Some(Ok(())) => tracing::info!("screenshot saved to {}", path.display()),
        Some(Err(e)) => tracing::warn!("screenshot not saved: {e}"),
        None => tracing::warn!("screenshot not saved: could not encode the frame"),
    }
}

/// Looks up a Ctrl+clicked file reference off the UI thread (a stat can be slow):
/// relative paths against the terminal's folder, then its project root.
pub(super) fn resolve_file_ref(
    wanted: String,
    line: Option<u32>,
    dir: Option<PathBuf>,
) -> Task<Message> {
    let key = wanted.clone();
    background(
        move || {
            let mut bases = Vec::new();
            if let Some(dir) = dir {
                let (root, _) = forgeterm_agents::project_root(&dir);
                bases.push(dir);
                if !bases.contains(&root) {
                    bases.push(root);
                }
            }
            forgeterm_terminal::resolve_file_ref(&key, &bases, Path::is_file)
        },
        move |found| Message::FileRefResolved {
            wanted: wanted.clone(),
            line,
            found,
        },
    )
}

pub(super) fn load_document(path: PathBuf) -> Task<Message> {
    let key = path.clone();
    background(
        move || forgeterm_viewer::load(&path).map(Box::new),
        move |result| Message::Viewer(ViewerMsg::Loaded(key.clone(), result)),
    )
}

pub(super) enum GitAction {
    Stage(Vec<String>),
    Unstage(Vec<String>),
    Commit(String),
    Switch(String),
    Track(String),
    Create(String),
}

pub(super) fn describe(paths: &[String]) -> String {
    match paths {
        [one] => one.clone(),
        many => format!("{} files", many.len()),
    }
}

/// One refresh: find the repository (unless known) and read its status (and log).
pub(super) fn git_snapshot(
    dir: PathBuf,
    known_root: Option<PathBuf>,
    with_log: bool,
    trusted: &[PathBuf],
) -> Snapshot {
    let root = match known_root.filter(|r| r.is_dir()) {
        Some(r) => Ok(Some(r)),
        None => forgeterm_git::discover(&dir),
    };
    let repo = match root {
        Err(e) => Repo::Error(e),
        Ok(None) => Repo::Outside,
        Ok(Some(root)) => {
            // Checked on every refresh: the repository's config can change.
            let programs = if trusted.contains(&root) {
                Ok(Vec::new())
            } else {
                forgeterm_git::repo_programs(&root)
            };
            match programs {
                Err(e) => Repo::Error(e),
                Ok(p) if !p.is_empty() => Repo::Untrusted { root, programs: p },
                Ok(_) => match forgeterm_git::status(&root) {
                    Ok(status) => Repo::Found { root, status },
                    Err(e) => Repo::Error(e),
                },
            }
        }
    };
    let (log, branches) = match (&repo, with_log) {
        (Repo::Found { root, .. }, true) => (
            Some(forgeterm_git::log(root, 15)),
            Some(forgeterm_git::branches(root)),
        ),
        _ => (None, None),
    };
    Snapshot {
        dir,
        repo,
        log,
        branches,
    }
}

/// Everything the Git Control Center shows besides the status (background thread).
/// Other worktrees are read like the main one: paused when their config names
/// programs and they are not trusted, and never when their folder is not local.
pub(super) fn load_dashboard(root: PathBuf, trusted: &[PathBuf]) -> DashData {
    let root_key = git_dash::path_key(&root);
    let read = |i: usize, worktree: &forgeterm_git::Worktree| -> WorktreeChanges {
        let path = &worktree.path;
        if git_dash::path_key(path) == root_key {
            WorktreeChanges::Current
        } else if worktree.bare || i >= git_dash::MAX_WORKTREE_STATUS {
            WorktreeChanges::NotRead
        } else if !path.to_str().is_some_and(forgeterm_terminal::is_local_path)
            || !forgeterm_fs::resolves_locally(path)
        {
            WorktreeChanges::NotLocal
        } else if !path.is_dir() {
            WorktreeChanges::Missing
        } else {
            match forgeterm_git::repo_programs(path) {
                Err(e) => WorktreeChanges::Error(e),
                Ok(p) if !p.is_empty() && !trusted.contains(path) => WorktreeChanges::Paused,
                Ok(_) => match forgeterm_git::status(path) {
                    Ok(s) => WorktreeChanges::Count {
                        changes: s.change_count(),
                        conflicts: s.conflicted().count(),
                    },
                    Err(e) => WorktreeChanges::Error(e),
                },
            }
        }
    };
    // In parallel: one slow worktree must not hold up the others.
    let worktrees = forgeterm_git::worktrees(&root).map(|list| {
        let changes: Vec<WorktreeChanges> = std::thread::scope(|scope| {
            let handles: Vec<_> = list
                .iter()
                .enumerate()
                .map(|(i, w)| scope.spawn(move || read(i, w)))
                .collect();
            handles
                .into_iter()
                .map(|h| {
                    h.join().unwrap_or_else(|_| {
                        WorktreeChanges::Error("reading it failed unexpectedly".to_owned())
                    })
                })
                .collect()
        });
        list.into_iter()
            .zip(changes)
            .map(|(worktree, changes)| WorktreeInfo { worktree, changes })
            .collect()
    });
    DashData {
        unstaged: forgeterm_git::numstat(&root, false),
        staged: forgeterm_git::numstat(&root, true),
        activity: forgeterm_git::activity(&root, 30),
        commits: forgeterm_git::history(&root, 8),
        branches: forgeterm_git::branches(&root),
        worktrees,
        root,
        loaded_at: std::time::Instant::now(),
    }
}

pub(super) fn load_prompts(dir: Option<PathBuf>) -> Task<Message> {
    background(
        move || forgeterm_prompts::load(dir.as_deref()),
        Message::PromptsLoaded,
    )
}

/// Example written when the prompts folder is created, so it is clear what goes in.
pub(super) const EXAMPLE_PROMPT: &str = "---
name: Example (edit me)
description: Your own template. Every .md file in this folder appears in the Prompt Studio.
---
Explain {{file}} in {{project}} in simple terms. Focus on: {{focus}}
";

/// Creates the prompts folder (with an example on first use) and opens it in the
/// system file manager.
pub(super) fn open_prompts_folder(dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        std::fs::write(dir.join("example.md"), EXAMPLE_PROMPT)
            .map_err(|e| format!("could not write the example template: {e}"))?;
    }
    let program = if cfg!(windows) {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(dir)
        .spawn()
        .map(drop)
        .map_err(|e| format!("could not open {} with {program}: {e}", dir.display()))
}

pub(super) fn scan_context(dir: PathBuf) -> Task<Message> {
    let key = dir.clone();
    background(
        move || forgeterm_agents::scan_context(&dir),
        move |ctx| Message::ContextScanned(key.clone(), ctx),
    )
}

pub(super) fn load_dir(path: PathBuf) -> Task<Message> {
    let target = path.clone();
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(list_dir(&target));
            });
            rx.await
                .unwrap_or_else(|_| Err("listing was interrupted".to_owned()))
        },
        move |result| Message::ExplorerListed(path.clone(), result),
    )
}

/// Takes the keyboard focus away from every text field (so keys reach the terminal):
/// iced's focus operation unfocuses every widget whose id differs from the target,
/// and no widget carries this id.
pub(super) fn unfocus_widgets() -> Task<Message> {
    iced::widget::operation::focus(iced::widget::Id::new("forgeterm-no-widget"))
}
