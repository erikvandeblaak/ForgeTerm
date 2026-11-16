//! Git Control Center: the repository of the focused terminal at a glance — branch
//! and upstream, what changed (with line counts), worktrees and the agents working
//! in them, activity, recent commits and branches. Read-only: staging and
//! committing stay in the Git panel, which can be open beside it.

use std::path::{Path, PathBuf};
use std::time::Instant;

use forgeterm_agents::AgentKind;
use forgeterm_git::{Activity, BranchRef, Change, Entry, FileStat, HistoryEntry, Status, Worktree};
use iced::widget::{column, container, mouse_area, row, scrollable, space, text};
use iced::{Border, Color, Element, Length};

use crate::app::Message;
use crate::commands::CommandId;
use crate::git_view::{self, Repo};
use crate::mission_view::{action_button, agent_badge, chip, stat_label, tag, tail, with_tip};
use crate::terminal_pane::TermId;
use crate::terminal_view::TERMINAL_FONT;
use crate::theme;

/// File rows in the working-tree card; the rest is counted, not drawn.
const MAX_FILES: usize = 150;
/// Worktrees whose status is read (each is a git process).
pub const MAX_WORKTREE_STATUS: usize = 12;

#[derive(Default)]
pub struct GitDash {
    pub visible: bool,
    pub busy: bool,
    pub data: Option<DashData>,
    /// The panel's status when `data` was loaded: a different status (a commit, a
    /// stage, an edit) means the line counts are out of date.
    pub loaded_for: Option<Status>,
    /// A forced reload was asked for while one was running: run it right after.
    pub reload_pending: bool,
}

/// One background load (see `App::load_git_dash`).
#[derive(Debug, Clone)]
pub struct DashData {
    pub root: PathBuf,
    pub worktrees: Result<Vec<WorktreeInfo>, String>,
    pub unstaged: Result<Vec<FileStat>, String>,
    pub staged: Result<Vec<FileStat>, String>,
    pub activity: Result<Activity, String>,
    pub commits: Result<Vec<HistoryEntry>, String>,
    pub branches: Result<Vec<BranchRef>, String>,
    pub loaded_at: Instant,
}

#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub worktree: Worktree,
    pub changes: WorktreeChanges,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeChanges {
    /// The repository shown: its status is the panel's.
    Current,
    Count {
        changes: usize,
        conflicts: usize,
    },
    /// Its config names programs (filters, …): not read until trusted.
    Paused,
    /// The folder does not exist.
    Missing,
    /// A network path: not read (it would connect to a server).
    NotLocal,
    NotRead,
    Error(String),
}

/// An agent terminal and where it works.
pub struct AgentAt {
    pub id: TermId,
    pub kind: AgentKind,
    pub tab: usize,
    pub dir: PathBuf,
}

/// Normalised for comparing paths from git (`C:/x`) with paths from the OS
/// (`C:\X\`): separators, trailing separators and (Windows) case.
pub fn path_key(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let s = s.trim_end_matches('/').to_owned();
    if cfg!(windows) { s.to_lowercase() } else { s }
}

/// Stable id of a worktree for commands (a hash of its normalised path).
pub fn worktree_id(path: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path_key(path).hash(&mut h);
    h.finish()
}

/// Index of the worktree that contains `dir` (the deepest one, since a worktree
/// can live inside another's folder).
pub fn worktree_of(dir: &Path, worktrees: &[PathBuf]) -> Option<usize> {
    let d = path_key(dir);
    worktrees
        .iter()
        .enumerate()
        .filter(|(_, w)| {
            let w = path_key(w);
            d == w || d.starts_with(&format!("{w}/"))
        })
        .max_by_key(|(_, w)| path_key(w).len())
        .map(|(i, _)| i)
}

/// Whether a worktree may be removed from ForgeTerm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Removal {
    /// Offered (git still refuses one with changes or untracked files).
    Allowed,
    /// A terminal of this window is in it: on Windows its folder cannot be deleted
    /// then (git would delete the files and fail on the folder), elsewhere the
    /// shell would sit in a deleted folder.
    InUse,
    /// The main or shown worktree, a bare or locked one, or one not read.
    Never,
}

/// The one rule for the "Remove…" button and `CommandId::RemoveWorktree`.
pub fn removal(is_main: bool, is_current: bool, info: &WorktreeInfo, terminals: usize) -> Removal {
    let readable = matches!(
        info.changes,
        WorktreeChanges::Count { .. } | WorktreeChanges::Current
    );
    let w = &info.worktree;
    if is_main || is_current || w.bare || w.locked.is_some() || !readable {
        Removal::Never
    } else if terminals > 0 {
        Removal::InUse
    } else {
        Removal::Allowed
    }
}

/// Lines added and removed by `path` in `stats`.
fn counts(stats: &Result<Vec<FileStat>, String>, path: &str) -> Option<(Option<u32>, Option<u32>)> {
    stats
        .as_ref()
        .ok()?
        .iter()
        .find(|s| s.path == path)
        .map(|s| (s.added, s.deleted))
}

fn totals(stats: &Result<Vec<FileStat>, String>) -> (u32, u32) {
    stats.as_ref().map_or((0, 0), |s| {
        s.iter().fold((0, 0), |(a, d), f| {
            (a + f.added.unwrap_or(0), d + f.deleted.unwrap_or(0))
        })
    })
}

pub struct View<'a> {
    pub repo: Option<&'a Repo>,
    pub dash: &'a GitDash,
    pub agents: Vec<AgentAt>,
    /// Folder of every terminal in this window (shells too): blocks removing.
    pub terminal_dirs: Vec<PathBuf>,
    /// Keys of the Git panel and of Compare, for hints (`None`: unbound).
    pub panel_hint: Option<String>,
    pub compare_hint: Option<String>,
    /// The focused terminal's folder (for the empty state).
    pub dir: Option<&'a Path>,
    pub now: Instant,
}

pub fn view(v: View<'_>) -> Element<'_, Message> {
    let body: Element<'_, Message> = match (v.repo, &v.dash.data) {
        (None, _) => message_card("Reading the repository…", None),
        (Some(Repo::Outside), _) => message_card(
            "The focused terminal is not in a Git repository",
            Some(format!(
                "{} — cd into a repository (or start a terminal there) and this page follows it.",
                v.dir
                    .map_or_else(|| "This folder".to_owned(), |d| d.display().to_string())
            )),
        ),
        (Some(Repo::Error(e)), _) => message_card("git reported an error", Some(e.clone())),
        (Some(Repo::Untrusted { root, programs }), _) => untrusted(root, programs),
        (Some(Repo::Found { root, status }), data) => {
            found(root, status, data.as_ref().filter(|d| &d.root == root), &v)
        }
    };
    container(
        scrollable(container(body).padding([18, 22]))
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(6).scroller_width(6),
            ))
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|_| container::Style {
        background: Some(theme::OBSIDIAN.into()),
        ..Default::default()
    })
    .into()
}

fn found<'a>(
    root: &'a Path,
    status: &'a Status,
    data: Option<&'a DashData>,
    v: &View<'a>,
) -> Element<'a, Message> {
    let mut page =
        column![header(root, status, data, v.now, v.compare_hint.as_deref())].spacing(16);
    page = page.push(tiles(status, data));
    let wt_paths: Vec<PathBuf> = data
        .and_then(|d| d.worktrees.as_ref().ok())
        .map(|w| w.iter().map(|i| i.worktree.path.clone()).collect())
        .unwrap_or_default();
    let main = row![
        container(working_tree(status, data)).width(Length::FillPortion(3)),
        container(worktrees_card(
            root,
            status,
            data,
            &wt_paths,
            &v.agents,
            &v.terminal_dirs,
        ))
        .width(Length::FillPortion(2)),
    ]
    .spacing(16);
    page = page.push(main);
    let lower = row![
        container(activity_card(data)).width(Length::FillPortion(3)),
        container(column![commits_card(data), branches_card(data)].spacing(16))
            .width(Length::FillPortion(2)),
    ]
    .spacing(16);
    page.push(lower)
        .push(
            text(format!(
                "An overview: only adding or removing a worktree (each asks first) changes \
                 the repository here. Stage and commit in the Git panel{}. Esc closes.",
                v.panel_hint
                    .as_deref()
                    .map(|k| format!(" ({k} opens it beside this page)"))
                    .unwrap_or_default()
            ))
            .size(11)
            .color(theme::DISABLED),
        )
        .into()
}

fn header<'a>(
    root: &'a Path,
    status: &'a Status,
    data: Option<&'a DashData>,
    now: Instant,
    compare_hint: Option<&str>,
) -> Element<'a, Message> {
    let name = root.file_name().map_or_else(
        || root.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let title = column![
        row![
            text("⎇").size(18).color(theme::VIOLET),
            text("Git Control Center").size(20).color(theme::TEXT),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center),
        row![
            text(name).size(13).color(theme::TEXT),
            container(
                text(tail(&root.display().to_string(), 46))
                    .size(11)
                    .color(theme::TEXT_MUTED)
                    .wrapping(text::Wrapping::None),
            )
            .width(Length::Fill)
            .clip(true),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center),
    ]
    .spacing(3);

    let b = &status.branch;
    let mut chips = row![chip(format!("⎇ {}", b.label()), theme::CYAN, true)]
        .spacing(8)
        .align_y(iced::Alignment::Center);
    chips = chips.push(match &b.upstream {
        Some(up) if b.ahead == 0 && b.behind == 0 => {
            chip(format!("in sync with {up}"), theme::MINT, true)
        }
        Some(up) => chip(
            format!("↑{} ↓{} {up}", b.ahead, b.behind),
            theme::AMBER,
            true,
        ),
        None => chip("no upstream".to_owned(), theme::TEXT_MUTED, false),
    });
    let conflicts = status.conflicted().count();
    if conflicts > 0 {
        chips = chips.push(chip(
            format!("{conflicts} conflict{}", plural(conflicts)),
            theme::CORAL,
            true,
        ));
    }
    let n = status.change_count();
    chips = chips.push(if n == 0 {
        chip("clean".to_owned(), theme::MINT, true)
    } else {
        chip(format!("{n} change{}", plural(n)), theme::AMBER, true)
    });

    let age = data.map_or_else(
        || "loading…".to_owned(),
        |d| {
            format!(
                "updated {} ago",
                forgeterm_agents::mission::format_duration(
                    now.saturating_duration_since(d.loaded_at)
                )
            )
        },
    );
    let actions = row![
        text(age).size(11).color(theme::TEXT_MUTED),
        action_button(
            "Refresh".to_owned(),
            Some(Message::Command(CommandId::GitRefresh)),
            theme::CYAN,
            false,
        ),
        action_button(
            "History…".to_owned(),
            Some(Message::Command(CommandId::GitShowHistory)),
            theme::CYAN,
            false,
        ),
        with_tip(
            action_button(
                "Agent in new worktree…".to_owned(),
                Some(Message::Command(CommandId::NewAgentWorktree)),
                theme::CYAN,
                false,
            ),
            "New branch in its own folder beside the repository, with an agent \
             started in it (asks first)"
                .to_owned(),
        ),
        with_tip(
            action_button(
                "Compare…".to_owned(),
                Some(Message::Command(CommandId::ToggleCompare)),
                theme::CYAN,
                false,
            ),
            format!(
                "Two worktrees side by side: changed files, diffs, tests{}",
                compare_hint.map(|k| format!(" ({k})")).unwrap_or_default()
            ),
        ),
        with_tip(
            action_button(
                "Stage & commit".to_owned(),
                Some(Message::Command(CommandId::ToggleGitPanel)),
                theme::CYAN,
                true,
            ),
            "Opens the Git panel beside this page".to_owned(),
        ),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    container(
        column![
            row![title, space::horizontal(), actions].align_y(iced::Alignment::Center),
            chips,
        ]
        .spacing(12),
    )
    .padding([16, 18])
    .width(Length::Fill)
    .style(card_style(theme::GRAPHITE))
    .into()
}

fn tiles<'a>(status: &'a Status, data: Option<&'a DashData>) -> Element<'a, Message> {
    let staged = status.staged().count();
    let unstaged = status.unstaged().count();
    let untracked = status.untracked().count();
    let lines = |stats: Option<&Result<Vec<FileStat>, String>>| match stats {
        None => "counting…".to_owned(),
        Some(Err(_)) => "line counts not available".to_owned(),
        Some(s) => {
            let (a, d) = totals(s);
            format!("+{a} −{d} lines")
        }
    };
    let commits = match data.map(|d| &d.activity) {
        None => ("…".to_owned(), "loading".to_owned()),
        Some(Ok(a)) => (
            a.total.to_string(),
            format!(
                "max {}/day",
                a.days.iter().map(|(_, n)| *n).max().unwrap_or(0)
            ),
        ),
        Some(Err(_)) => ("—".to_owned(), "not available".to_owned()),
    };
    let wts = match data.map(|d| &d.worktrees) {
        None => ("…".to_owned(), "loading".to_owned()),
        Some(Ok(w)) => (
            w.len().to_string(),
            if w.len() > 1 {
                "linked worktrees in use"
            } else {
                "main only"
            }
            .to_owned(),
        ),
        Some(Err(_)) => ("—".to_owned(), "not available".to_owned()),
    };
    row![
        tile(
            "STAGED",
            staged.to_string(),
            lines(data.map(|d| &d.staged)),
            theme::MINT
        ),
        tile(
            "UNSTAGED",
            unstaged.to_string(),
            lines(data.map(|d| &d.unstaged)),
            theme::AMBER
        ),
        tile(
            "UNTRACKED",
            untracked.to_string(),
            "new files".to_owned(),
            theme::VIOLET
        ),
        tile("COMMITS · 30 DAYS", commits.0, commits.1, theme::CHART_CYAN),
        tile("WORKTREES", wts.0, wts.1, theme::CYAN),
    ]
    .spacing(12)
    .into()
}

pub(crate) fn tile<'a>(
    label: &'a str,
    value: String,
    sub: String,
    accent: Color,
) -> Element<'a, Message> {
    container(
        column![
            row![
                container(space::horizontal())
                    .width(8)
                    .height(8)
                    .style(move |_| container::Style {
                        background: Some(accent.into()),
                        border: Border {
                            radius: 2.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                stat_label(label),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
            text(value).size(24).color(theme::TEXT),
            text(sub).size(11).color(theme::TEXT_MUTED),
        ]
        .spacing(4),
    )
    .padding([12, 14])
    .width(Length::Fill)
    .style(card_style(theme::SURFACE))
    .into()
}

#[derive(Clone, Copy)]
enum Group {
    Conflict,
    Staged,
    Unstaged,
    Untracked,
}

fn working_tree<'a>(status: &'a Status, data: Option<&'a DashData>) -> Element<'a, Message> {
    let mut list = column![card_title("WORKING TREE")].spacing(4);
    if status.change_count() == 0 {
        list = list.push(
            container(
                column![
                    text("✓").size(22).color(theme::MINT),
                    text("Working tree clean").size(13).color(theme::TEXT),
                    text("Nothing to stage or commit.")
                        .size(11)
                        .color(theme::TEXT_MUTED),
                ]
                .spacing(4)
                .align_x(iced::Alignment::Center),
            )
            .center_x(Length::Fill)
            .padding(24),
        );
        return card(list);
    }
    // The widest change sets the scale of the bars.
    let max_lines = data
        .map(|d| {
            [&d.staged, &d.unstaged]
                .iter()
                .filter_map(|s| s.as_ref().ok())
                .flatten()
                .map(|f| f.added.unwrap_or(0) + f.deleted.unwrap_or(0))
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0)
        .max(1);
    let groups: [(Group, &str, Vec<&Entry>); 4] = [
        (Group::Conflict, "CONFLICTS", status.conflicted().collect()),
        (Group::Staged, "STAGED", status.staged().collect()),
        (Group::Unstaged, "UNSTAGED", status.unstaged().collect()),
        (Group::Untracked, "UNTRACKED", status.untracked().collect()),
    ];
    let mut shown = 0;
    let mut hidden = 0;
    for (group, title, entries) in groups {
        if entries.is_empty() {
            continue;
        }
        list = list.push(
            container(
                text(format!("{title} · {}", entries.len()))
                    .size(10)
                    .color(theme::TEXT_MUTED),
            )
            .padding([6, 0]),
        );
        for e in entries {
            if shown >= MAX_FILES {
                hidden += 1;
                continue;
            }
            shown += 1;
            list = list.push(file_row(e, group, data, max_lines));
        }
    }
    if hidden > 0 {
        list = list.push(
            text(format!(
                "… and {hidden} more (not drawn; the Git panel lists them too)"
            ))
            .size(11)
            .color(theme::TEXT_MUTED),
        );
    }
    card(list)
}

fn file_row<'a>(
    e: &'a Entry,
    group: Group,
    data: Option<&'a DashData>,
    max_lines: u32,
) -> Element<'a, Message> {
    use forgeterm_git::DiffKind;
    let (letter, color, kind, stats) = match group {
        Group::Conflict => ('!', theme::CORAL, DiffKind::Unstaged, None),
        Group::Staged => (
            e.staged.map_or('?', Change::letter),
            theme::MINT,
            DiffKind::Staged,
            data.map(|d| &d.staged),
        ),
        Group::Unstaged => (
            e.unstaged.map_or('?', Change::letter),
            theme::AMBER,
            DiffKind::Unstaged,
            data.map(|d| &d.unstaged),
        ),
        Group::Untracked => ('U', theme::VIOLET, DiffKind::Untracked, None),
    };
    let (dir, file) = match e.path.rsplit_once('/') {
        Some((d, f)) => (d, f),
        None => ("", e.path.as_str()),
    };
    let name = row![
        text(letter.to_string())
            .size(12)
            .font(TERMINAL_FONT)
            .color(color),
        text(file)
            .size(12)
            .color(theme::TEXT)
            .wrapping(text::Wrapping::None),
        text(dir)
            .size(10)
            .color(theme::TEXT_MUTED)
            .wrapping(text::Wrapping::None),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    let (bar, label): (Element<'a, Message>, String) = match stats.and_then(|s| counts(s, &e.path))
    {
        Some((Some(a), Some(d))) => (diff_bar(a, d, max_lines), format!("+{a} −{d}")),
        Some(_) => (space::horizontal().width(0).into(), "binary".to_owned()),
        None if matches!(group, Group::Untracked) => {
            (space::horizontal().width(0).into(), "new".to_owned())
        }
        None => (space::horizontal().width(0).into(), String::new()),
    };
    let line = row![
        container(name).width(Length::Fill).clip(true),
        container(bar)
            .width(120)
            .align_x(iced::alignment::Horizontal::Right),
        container(
            text(label)
                .size(11)
                .font(TERMINAL_FONT)
                .color(theme::TEXT_MUTED)
        )
        .width(90)
        .align_x(iced::alignment::Horizontal::Right),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center);
    let tip = match &e.orig_path {
        Some(orig) => format!("{orig} → {}\nClick to show the diff", e.path),
        None => format!("{}\nClick to show the diff", e.path),
    };
    with_tip(
        mouse_area(container(line).padding([3, 6]).width(Length::Fill))
            .on_press(Message::GitShowDiff(e.path.clone(), kind))
            .interaction(iced::mouse::Interaction::Pointer),
        tip,
    )
}

/// Green/red split bar, its length relative to the biggest change in the list.
pub(crate) fn diff_bar<'a>(added: u32, deleted: u32, max: u32) -> Element<'a, Message> {
    let full = 110.0;
    let total = (added + deleted) as f32;
    let width = (full * total / max as f32).max(if total > 0.0 { 4.0 } else { 0.0 });
    let green = if total > 0.0 {
        width * added as f32 / total
    } else {
        0.0
    };
    let seg = |w: f32, c: Color| {
        container(space::horizontal())
            .width(w)
            .height(6)
            .style(move |_| container::Style {
                background: Some(c.into()),
                border: Border {
                    radius: 2.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
    };
    row![seg(green, theme::MINT), seg(width - green, theme::CORAL)]
        .spacing(1)
        .into()
}

fn worktrees_card<'a>(
    root: &'a Path,
    status: &'a Status,
    data: Option<&'a DashData>,
    wt_paths: &[PathBuf],
    agents: &[AgentAt],
    terminal_dirs: &[PathBuf],
) -> Element<'a, Message> {
    let mut list = column![card_title("WORKTREES")].spacing(10);
    let worktrees = match data.map(|d| &d.worktrees) {
        None => return card(list.push(muted("Loading…"))),
        Some(Err(e)) => return card(list.push(muted(&format!("Not available: {e}")))),
        Some(Ok(w)) => w,
    };
    // Which agent works in which worktree: by the folder ForgeTerm started it in.
    let mut per: Vec<Vec<&AgentAt>> = vec![Vec::new(); worktrees.len()];
    let mut elsewhere = 0;
    for a in agents {
        match worktree_of(&a.dir, wt_paths) {
            Some(i) => per[i].push(a),
            None => elsewhere += 1,
        }
    }
    let mut terminals = vec![0usize; worktrees.len()];
    for d in terminal_dirs {
        if let Some(i) = worktree_of(d, wt_paths) {
            terminals[i] += 1;
        }
    }
    let root_key = path_key(root);
    for (i, info) in worktrees.iter().enumerate() {
        let is_current = path_key(&info.worktree.path) == root_key;
        list = list.push(worktree_row(
            removal(i == 0, is_current, info, terminals[i]),
            info,
            is_current,
            status,
            &per[i],
        ));
    }
    if worktrees.len() == 1 {
        list = list.push(muted(
            "Only the main worktree. Agents can work side by side in linked worktrees \
             (git worktree add), each on its own branch.",
        ));
    }
    if elsewhere > 0 {
        list = list.push(muted(&format!(
            "{elsewhere} agent{} in other folders (see Mission Control).",
            plural(elsewhere)
        )));
    }
    card(list)
}

fn worktree_row<'a>(
    removal: Removal,
    info: &'a WorktreeInfo,
    is_current: bool,
    status: &'a Status,
    agents: &[&AgentAt],
) -> Element<'a, Message> {
    let w = &info.worktree;
    let name = w.path.file_name().map_or_else(
        || w.path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let branch = match (&w.branch, &w.head) {
        (Some(b), _) => format!("⎇ {b}"),
        (None, Some(h)) if w.detached => format!("detached @ {}", &h[..h.len().min(8)]),
        _ if w.bare => "bare".to_owned(),
        _ => "—".to_owned(),
    };
    let mut title = row![
        text(name).size(13).color(theme::TEXT),
        text(branch).size(12).color(theme::VIOLET),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);
    if is_current {
        title = title.push(tag("shown", theme::CYAN));
    }
    if w.locked.is_some() {
        title = title.push(tag("locked", theme::AMBER));
    }
    if w.prunable.is_some() {
        title = title.push(tag("prunable", theme::CORAL));
    }
    let (changes, color) = match &info.changes {
        WorktreeChanges::Current => {
            let n = status.change_count();
            let c = status.conflicted().count();
            changes_label(n, c)
        }
        WorktreeChanges::Count { changes, conflicts } => changes_label(*changes, *conflicts),
        WorktreeChanges::Paused => (
            "not read: its config runs programs (trust it in the Git panel)".to_owned(),
            theme::AMBER,
        ),
        WorktreeChanges::Missing => ("folder not found".to_owned(), theme::CORAL),
        WorktreeChanges::NotLocal => ("network folder: not read".to_owned(), theme::TEXT_MUTED),
        WorktreeChanges::NotRead => (
            "not read (too many worktrees)".to_owned(),
            theme::TEXT_MUTED,
        ),
        WorktreeChanges::Error(e) => (format!("git: {e}"), theme::CORAL),
    };
    let mut col = column![
        title,
        container(
            text(tail(&w.path.display().to_string(), 60))
                .size(11)
                .color(theme::TEXT_MUTED)
                .wrapping(text::Wrapping::None),
        )
        .width(Length::Fill)
        .clip(true),
        text(changes).size(12).color(color),
    ]
    .spacing(3);
    let mut actions = row![].spacing(6).align_y(iced::Alignment::Center);
    for a in agents {
        actions = actions.push(with_tip(
            mouse_area(
                row![
                    agent_badge(a.kind),
                    text(format!("{} · tab {}", a.kind.name(), a.tab + 1))
                        .size(11)
                        .color(theme::TEXT),
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::Command(CommandId::GoToTerminal(a.id)))
            .interaction(iced::mouse::Interaction::Pointer),
            format!(
                "{} works in this worktree — click to open its terminal",
                a.kind.name()
            ),
        ));
    }
    if !w.bare
        && !matches!(
            info.changes,
            WorktreeChanges::Missing | WorktreeChanges::NotLocal
        )
    {
        actions = actions.push(space::horizontal());
        actions = actions.push(with_tip(
            action_button(
                "Terminal here".to_owned(),
                Some(Message::Command(CommandId::WorktreeTerminal(worktree_id(
                    &w.path,
                )))),
                theme::CYAN,
                false,
            ),
            "New tab with your default shell in this worktree".to_owned(),
        ));
        // A linked worktree no terminal here is in can go (git refuses a dirty one).
        let (press, tip) = match removal {
            Removal::Never => (None, None),
            Removal::InUse => (
                None,
                Some("A terminal of this window is in this worktree: close it first"),
            ),
            Removal::Allowed => (
                Some(Message::Command(CommandId::RemoveWorktree(worktree_id(
                    &w.path,
                )))),
                Some(
                    "Removes the folder (asks first; git refuses if it has changes). \
                     The branch is kept.",
                ),
            ),
        };
        if let Some(tip) = tip {
            actions = actions.push(with_tip(
                action_button("Remove…".to_owned(), press, theme::CORAL, false),
                tip.to_owned(),
            ));
        }
    }
    col = col.push(actions);
    container(col)
        .padding([10, 12])
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(theme::GRAPHITE.into()),
            border: Border {
                color: if is_current {
                    Color {
                        a: 0.5,
                        ..theme::CYAN
                    }
                } else {
                    theme::BORDER
                },
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn changes_label(changes: usize, conflicts: usize) -> (String, Color) {
    if conflicts > 0 {
        (
            format!(
                "{conflicts} conflict{}, {changes} change{}",
                plural(conflicts),
                plural(changes)
            ),
            theme::CORAL,
        )
    } else if changes > 0 {
        (format!("{changes} change{}", plural(changes)), theme::AMBER)
    } else {
        ("clean".to_owned(), theme::MINT)
    }
}

fn activity_card(data: Option<&DashData>) -> Element<'_, Message> {
    let inner: Element<'_, Message> = match data.map(|d| &d.activity) {
        None => muted("Loading…"),
        Some(Err(e)) => muted_owned(format!("Not available: {e}")),
        Some(Ok(a)) => git_view::activity_chart(a),
    };
    card(column![card_title("ACTIVITY"), inner].spacing(10))
}

fn commits_card(data: Option<&DashData>) -> Element<'_, Message> {
    let mut list = column![card_title("RECENT COMMITS")].spacing(6);
    match data.map(|d| &d.commits) {
        None => list = list.push(muted("Loading…")),
        Some(Err(e)) => list = list.push(muted_owned(format!("Not available: {e}"))),
        Some(Ok(c)) if c.is_empty() => list = list.push(muted("No commits yet.")),
        Some(Ok(c)) => {
            for e in c.iter().take(8) {
                list = list.push(
                    row![
                        text(e.short.clone())
                            .size(11)
                            .font(TERMINAL_FONT)
                            .color(theme::CYAN),
                        column![
                            text(e.subject.clone())
                                .size(12)
                                .color(theme::TEXT)
                                .wrapping(text::Wrapping::None),
                            text(format!("{} · {}", e.author, e.when))
                                .size(10)
                                .color(theme::TEXT_MUTED),
                        ]
                        .spacing(1),
                    ]
                    .spacing(10),
                );
            }
        }
    }
    card(list)
}

fn branches_card(data: Option<&DashData>) -> Element<'_, Message> {
    let mut list = column![card_title("LOCAL BRANCHES")].spacing(6);
    match data.map(|d| &d.branches) {
        None => list = list.push(muted("Loading…")),
        Some(Err(e)) => list = list.push(muted_owned(format!("Not available: {e}"))),
        Some(Ok(b)) => {
            let local: Vec<&BranchRef> = b.iter().filter(|b| !b.remote).collect();
            for br in local.iter().take(8) {
                let mut r = row![
                    text(if br.current { "●" } else { "○" })
                        .size(10)
                        .color(if br.current {
                            theme::CYAN
                        } else {
                            theme::DISABLED
                        }),
                    text(br.name.clone())
                        .size(12)
                        .color(if br.current { theme::CYAN } else { theme::TEXT })
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center);
                r = r.push(space::horizontal());
                if br.upstream_gone {
                    r = r.push(text("upstream gone").size(10).color(theme::CORAL));
                } else if br.ahead > 0 || br.behind > 0 {
                    r = r.push(
                        text(format!("↑{} ↓{}", br.ahead, br.behind))
                            .size(11)
                            .color(theme::AMBER),
                    );
                }
                list = list.push(r);
            }
            if local.len() > 8 {
                list = list.push(muted_owned(format!("… and {} more", local.len() - 8)));
            }
        }
    }
    card(list)
}

pub(crate) fn untrusted<'a>(root: &'a Path, programs: &'a [String]) -> Element<'a, Message> {
    let mut col = column![
        text("Git is paused for this repository")
            .size(18)
            .color(theme::AMBER),
        text(tail(&root.display().to_string(), 90))
            .size(12)
            .color(theme::TEXT_MUTED)
            .wrapping(text::Wrapping::None),
        text(
            "Its own config makes git run these programs. ForgeTerm reads nothing \
             automatically until you trust it:"
        )
        .size(12)
        .color(theme::TEXT),
    ]
    .spacing(8);
    for p in programs.iter().take(12) {
        col = col.push(
            text(p.clone())
                .size(11)
                .font(TERMINAL_FONT)
                .color(theme::TEXT_MUTED),
        );
    }
    col = col.push(action_button(
        "Trust this repository".to_owned(),
        Some(Message::Command(CommandId::GitTrustRepository)),
        theme::AMBER,
        true,
    ));
    container(col)
        .padding(20)
        .max_width(720)
        .style(card_style(theme::SURFACE))
        .into()
}

pub(crate) fn message_card<'a>(title: &str, detail: Option<String>) -> Element<'a, Message> {
    let mut col = column![text(title.to_owned()).size(18).color(theme::TEXT)].spacing(8);
    if let Some(d) = detail {
        col = col.push(text(d).size(12).color(theme::TEXT_MUTED));
    }
    container(col)
        .padding(20)
        .max_width(720)
        .style(card_style(theme::SURFACE))
        .into()
}

pub(crate) fn card<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .padding([14, 16])
        .width(Length::Fill)
        .style(card_style(theme::SURFACE))
        .into()
}

pub(crate) fn card_style(bg: Color) -> impl Fn(&iced::Theme) -> container::Style {
    move |_| container::Style {
        background: Some(bg.into()),
        border: Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    }
}

pub(crate) fn card_title<'a>(s: &'a str) -> Element<'a, Message> {
    text(s).size(10).color(theme::TEXT_MUTED).into()
}

pub(crate) fn muted<'a>(s: &str) -> Element<'a, Message> {
    text(s.to_owned()).size(11).color(theme::TEXT_MUTED).into()
}

pub(crate) fn muted_owned<'a>(s: String) -> Element<'a, Message> {
    text(s).size(11).color(theme::TEXT_MUTED).into()
}

pub(crate) fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agents_are_matched_to_the_deepest_worktree() {
        let wts = vec![
            PathBuf::from("C:/work/shop"),
            PathBuf::from("C:/work/shop/.worktrees/claude"),
            PathBuf::from("C:/work/shop-codex"),
        ];
        let at = |p: &str| worktree_of(Path::new(p), &wts);
        assert_eq!(at("C:/work/shop"), Some(0));
        assert_eq!(at("C:/work/shop/src"), Some(0));
        assert_eq!(at("C:/work/shop/.worktrees/claude"), Some(1));
        assert_eq!(at("C:/work/shop/.worktrees/claude/src/"), Some(1));
        // A sibling folder with the same prefix is not inside `shop`.
        assert_eq!(at("C:/work/shop-codex"), Some(2));
        assert_eq!(at("C:/work/shopping"), None);
        assert_eq!(at("D:/elsewhere"), None);
    }

    #[test]
    #[cfg(windows)]
    fn windows_paths_compare_without_case_or_separator_differences() {
        let wts = vec![PathBuf::from("C:/Work/Shop")];
        assert_eq!(worktree_of(Path::new(r"c:\work\shop\src"), &wts), Some(0));
        assert_eq!(path_key(Path::new(r"C:\A\B\")), "c:/a/b");
    }

    #[test]
    fn only_idle_linked_worktrees_can_be_removed() {
        let info = |changes: WorktreeChanges, bare: bool, locked: Option<&str>| WorktreeInfo {
            worktree: Worktree {
                path: PathBuf::from("C:/work/shop-worktrees/agent-claude-1"),
                head: Some("abc".to_owned()),
                branch: Some("agent/claude-1".to_owned()),
                detached: false,
                bare,
                locked: locked.map(str::to_owned),
                prunable: None,
            },
            changes,
        };
        let clean = info(
            WorktreeChanges::Count {
                changes: 0,
                conflicts: 0,
            },
            false,
            None,
        );
        assert_eq!(removal(false, false, &clean, 0), Removal::Allowed);
        // Any terminal of this window in it (a shell too) blocks it.
        assert_eq!(removal(false, false, &clean, 1), Removal::InUse);
        // Never the main or the shown worktree.
        assert_eq!(removal(true, false, &clean, 0), Removal::Never);
        assert_eq!(removal(false, true, &clean, 0), Removal::Never);
        // Never bare, locked, or one whose state was not read.
        assert_eq!(
            removal(
                false,
                false,
                &info(
                    WorktreeChanges::Count {
                        changes: 0,
                        conflicts: 0
                    },
                    true,
                    None
                ),
                0
            ),
            Removal::Never
        );
        assert_eq!(
            removal(
                false,
                false,
                &info(
                    WorktreeChanges::Count {
                        changes: 0,
                        conflicts: 0
                    },
                    false,
                    Some("")
                ),
                0
            ),
            Removal::Never
        );
        for c in [
            WorktreeChanges::Paused,
            WorktreeChanges::Missing,
            WorktreeChanges::NotLocal,
            WorktreeChanges::NotRead,
            WorktreeChanges::Error("x".to_owned()),
        ] {
            assert_eq!(
                removal(false, false, &info(c, false, None), 0),
                Removal::Never
            );
        }
    }

    #[test]
    fn worktree_ids_are_stable_and_distinct() {
        let a = worktree_id(Path::new("C:/work/shop"));
        assert_eq!(a, worktree_id(Path::new("C:/work/shop/")));
        assert_ne!(a, worktree_id(Path::new("C:/work/shop-claude")));
    }

    #[test]
    fn totals_and_counts() {
        let stats: Result<Vec<FileStat>, String> = Ok(vec![
            FileStat {
                path: "a".into(),
                added: Some(3),
                deleted: Some(1),
            },
            FileStat {
                path: "img.png".into(),
                added: None,
                deleted: None,
            },
            FileStat {
                path: "b".into(),
                added: Some(0),
                deleted: Some(4),
            },
        ]);
        assert_eq!(totals(&stats), (3, 5));
        assert_eq!(counts(&stats, "a"), Some((Some(3), Some(1))));
        assert_eq!(counts(&stats, "img.png"), Some((None, None)));
        assert_eq!(counts(&stats, "zzz"), None);
        assert_eq!(totals(&Err("x".into())), (0, 0));
    }
}
