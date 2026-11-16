//! Git side panel (status, stage/unstage, commit, recent commits), the diff overlay
//! and the status-bar summary. All git work happens in `forgeterm-git` on background
//! threads; this module only holds what came back and draws it.

use std::path::PathBuf;

use forgeterm_git::{
    Activity, BranchRef, Change, Commit, Diff, DiffKind, Entry, HistoryEntry, Status,
};
use iced::widget::{
    button, column, container, mouse_area, opaque, row, scrollable, space, text, text_editor,
    text_input, tooltip,
};
use iced::{Border, Color, Element, Length, Shadow, Vector};

use crate::app::Message;
use crate::commands::CommandId;
use crate::terminal_view::TERMINAL_FONT;
use crate::theme;

pub const WIDTH: f32 = 300.0;
/// Rows shown per section; the rest is summarised (a huge `node_modules` checkout
/// must not create thousands of widgets).
const MAX_ROWS: usize = 150;

/// What git said about a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repo {
    /// The folder is not inside a repository.
    Outside,
    /// The repository's own config makes git run programs (filters, …): automatic
    /// reads are paused until the user trusts it.
    Untrusted {
        root: PathBuf,
        programs: Vec<String>,
    },
    /// git could not be run or failed (message from git).
    Error(String),
    Found {
        root: PathBuf,
        status: Status,
    },
}

/// Result of one background refresh.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub dir: PathBuf,
    pub repo: Repo,
    /// Only fetched while the panel is visible.
    pub log: Option<Result<Vec<Commit>, String>>,
    /// Only fetched while the panel is visible.
    pub branches: Option<Result<Vec<BranchRef>, String>>,
}

/// The history overlay: commit list, activity, one commit's details.
#[derive(Debug, Default)]
pub struct HistoryView {
    /// `None` while loading.
    pub entries: Option<Result<Vec<HistoryEntry>, String>>,
    pub activity: Option<Result<Activity, String>>,
    pub selected: Option<String>,
    /// Details of `selected`; `None` while loading.
    pub detail: Option<Result<Diff, String>>,
}

#[derive(Debug)]
pub struct DiffView {
    pub path: String,
    pub kind: DiffKind,
    /// `None` while loading.
    pub result: Option<Result<Diff, String>>,
}

#[derive(Default)]
pub struct GitState {
    /// Folder the shown state belongs to.
    pub dir: Option<PathBuf>,
    pub repo: Option<Repo>,
    pub log: Option<Result<Vec<Commit>, String>>,
    /// A refresh is running (for this folder).
    pub refreshing: Option<PathBuf>,
    /// A stage/unstage/commit is running.
    pub action_running: bool,
    /// The running (or last) action is a commit: clear the message on success.
    pub last_action_was_commit: bool,
    /// The running (or last) action creates a branch: clear the typed name on
    /// success only, so a refused name can be corrected.
    pub last_action_was_create: bool,
    /// Outcome of the last action, shown until the next one.
    pub last_action: Option<Result<String, String>>,
    pub commit_message: text_editor::Content,
    pub diff: Option<DiffView>,
    pub branches: Option<Result<Vec<BranchRef>, String>>,
    /// Branch the user clicked "Switch" for; switching waits for confirmation.
    pub pending_switch: Option<String>,
    pub show_remotes: bool,
    pub new_branch: String,
    pub history: Option<HistoryView>,
    /// Repositories the user trusted in this session despite their own programs.
    pub trusted: std::collections::HashSet<PathBuf>,
}

impl GitState {
    pub fn root(&self) -> Option<&PathBuf> {
        match &self.repo {
            Some(Repo::Found { root, .. }) => Some(root),
            _ => None,
        }
    }

    pub fn status(&self) -> Option<&Status> {
        match &self.repo {
            Some(Repo::Found { status, .. }) => Some(status),
            _ => None,
        }
    }

    pub fn apply(&mut self, snapshot: Snapshot) {
        let new_root = match &snapshot.repo {
            Repo::Found { root, .. } => Some(root),
            _ => None,
        };
        // An action message belongs to the repository it was done in.
        if self.root() != new_root {
            self.last_action = None;
            self.pending_switch = None;
            self.branches = None;
        }
        self.dir = Some(snapshot.dir);
        self.repo = Some(snapshot.repo);
        if snapshot.log.is_some() {
            self.log = snapshot.log;
        }
        if snapshot.branches.is_some() {
            self.branches = snapshot.branches;
        }
        if self.root().is_none() {
            self.log = None;
            self.branches = None;
        }
    }

    /// Status-bar text: `main ↑1 ↓2 · 3 changed`; `None` outside a repository.
    pub fn summary(&self) -> Option<(String, Color)> {
        if let Some(Repo::Untrusted { .. }) = &self.repo {
            return Some((
                "git paused (repository runs programs)".into(),
                theme::amber(),
            ));
        }
        let status = self.status()?;
        let b = &status.branch;
        let mut s = b.label();
        if b.ahead > 0 {
            s.push_str(&format!(" ↑{}", b.ahead));
        }
        if b.behind > 0 {
            s.push_str(&format!(" ↓{}", b.behind));
        }
        let conflicts = status.conflicted().count();
        let color = if conflicts > 0 {
            s.push_str(&format!(" · {conflicts} conflict{}", plural(conflicts)));
            theme::coral()
        } else if status.change_count() > 0 {
            s.push_str(&format!(" · {} changed", status.change_count()));
            theme::amber()
        } else {
            s.push_str(" · clean");
            theme::mint()
        };
        Some((s, color))
    }

    /// Every changed (unstaged) and untracked file, for "Stage All Changes".
    pub fn stage_all_paths(&self) -> Vec<String> {
        self.status().map_or_else(Vec::new, |s| {
            s.unstaged()
                .chain(s.untracked())
                .map(|e| e.path.clone())
                .collect()
        })
    }

    /// Every staged file, for "Unstage All".
    pub fn unstage_all_paths(&self) -> Vec<String> {
        self.status()
            .map_or_else(Vec::new, |s| s.staged().map(|e| e.path.clone()).collect())
    }

    pub fn can_commit(&self) -> bool {
        !self.action_running
            && !self.commit_message.text().trim().is_empty()
            && self
                .status()
                .is_some_and(|s| s.staged().next().is_some() && s.conflicted().next().is_none())
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

pub fn view(git: &GitState) -> Element<'_, Message> {
    let header = row![
        text("GIT")
            .size(theme::size::TINY)
            .color(theme::text_muted()),
        space::horizontal(),
        small_button(
            "↻",
            Some(Message::Command(CommandId::GitRefresh)),
            "Refresh"
        ),
    ]
    .align_y(iced::Alignment::Center);

    let mut body = column![].spacing(theme::space::L);
    match &git.repo {
        None => body = body.push(muted("Reading…")),
        Some(Repo::Outside) => {
            body = body.push(muted("Not a Git repository:"));
            if let Some(dir) = &git.dir {
                body = body.push(path_text(dir.display().to_string()));
            }
        }
        Some(Repo::Untrusted { programs, .. }) => {
            body = body.push(
                text("Automatic Git status is paused for this repository.")
                    .size(theme::size::BODY)
                    .color(theme::amber()),
            );
            body = body.push(muted(
                "Its own configuration (.git/config) makes git run these programs, \
                 which reading the status could start:",
            ));
            for p in programs.iter().take(8) {
                body = body.push(
                    text(p.as_str())
                        .size(theme::size::TINY)
                        .font(TERMINAL_FONT)
                        .color(theme::text())
                        .wrapping(text::Wrapping::Glyph),
                );
            }
            body = body.push(muted(
                "Only trust it if you know this repository (for example your own). \
                 The choice lasts until ForgeTerm closes.",
            ));
            body = body.push(small_button(
                "Trust this repository",
                Some(Message::Command(CommandId::GitTrustRepository)),
                "Run git status here, including the programs above",
            ));
        }
        Some(Repo::Error(e)) => {
            body = body.push(
                text(e.as_str())
                    .size(theme::size::SMALL)
                    .color(theme::coral()),
            );
        }
        Some(Repo::Found { root, status }) => {
            body = body.push(branch_block(root, status));
            if let Some(result) = &git.last_action {
                body = body.push(match result {
                    Ok(m) => text(m.as_str())
                        .size(theme::size::SMALL)
                        .color(theme::mint()),
                    Err(e) => text(e.as_str())
                        .size(theme::size::SMALL)
                        .color(theme::coral()),
                });
            }
            body = body.push(section(
                "CONFLICTS",
                status.conflicted(),
                RowKind::Conflict,
                git.action_running,
            ));
            body = body.push(section(
                "STAGED",
                status.staged(),
                RowKind::Staged,
                git.action_running,
            ));
            body = body.push(section(
                "CHANGES",
                status.unstaged(),
                RowKind::Unstaged,
                git.action_running,
            ));
            body = body.push(section(
                "UNTRACKED",
                status.untracked(),
                RowKind::Untracked,
                git.action_running,
            ));
            if status.change_count() == 0 {
                body = body.push(muted("Working tree clean."));
            }
            body = body.push(commit_box(git, status));
            body = body.push(branches_block(git));
            body = body.push(log_block(git.log.as_ref()));
            body = body.push(small_button(
                "History & activity…",
                Some(Message::Command(CommandId::GitShowHistory)),
                "All recent commits, their changes, and commits per day",
            ));
        }
    }

    container(
        column![
            header,
            // Right padding keeps the scrollbar off the buttons.
            scrollable(body.padding(iced::Padding {
                top: 0.0,
                right: 16.0,
                bottom: 0.0,
                left: 8.0,
            }))
            .height(Length::Fill)
        ]
        .spacing(theme::space::M),
    )
    .padding([10, 10])
    .width(Length::Fixed(WIDTH))
    .height(Length::Fill)
    .style(|_| container::Style {
        background: Some(theme::graphite().into()),
        border: Border {
            color: theme::border(),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn branch_block<'a>(root: &'a std::path::Path, status: &'a Status) -> Element<'a, Message> {
    let b = &status.branch;
    let name = root.file_name().map_or_else(
        || root.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let upstream = match &b.upstream {
        Some(u) => format!("{u} · ↑{} ↓{}", b.ahead, b.behind),
        None if b.has_commits() => "no upstream branch".to_owned(),
        None => "no commits yet".to_owned(),
    };
    column![
        row![
            text(b.label())
                .size(theme::size::LARGE)
                .color(theme::cyan()),
            space::horizontal(),
            text(name)
                .size(theme::size::TINY)
                .color(theme::text_muted()),
        ]
        .align_y(iced::Alignment::Center),
        text(upstream)
            .size(theme::size::TINY)
            .color(theme::text_muted()),
    ]
    .spacing(theme::space::XXS)
    .into()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowKind {
    Conflict,
    Staged,
    Unstaged,
    Untracked,
}

fn section<'a>(
    title: &'static str,
    entries: impl Iterator<Item = &'a Entry>,
    kind: RowKind,
    busy: bool,
) -> Element<'a, Message> {
    let entries: Vec<&Entry> = entries.collect();
    if entries.is_empty() {
        return space::vertical().height(0).into();
    }
    let paths: Vec<String> = entries.iter().map(|e| e.path.clone()).collect();
    let all_action = match kind {
        RowKind::Staged => Some(("− all", Message::GitUnstage(paths), "Unstage all")),
        RowKind::Unstaged | RowKind::Untracked => {
            Some(("+ all", Message::GitStage(paths), "Stage all"))
        }
        RowKind::Conflict => None,
    };
    let mut head = row![
        text(format!("{title} ({})", entries.len()))
            .size(theme::size::TINY)
            .color(if kind == RowKind::Conflict {
                theme::coral()
            } else {
                theme::text_muted()
            }),
        space::horizontal(),
    ]
    .align_y(iced::Alignment::Center);
    if let Some((label, msg, tip)) = all_action {
        head = head.push(small_button(label, (!busy).then_some(msg), tip));
    }
    let mut list = column![head].spacing(theme::space::XXS);
    for e in entries.iter().take(MAX_ROWS) {
        list = list.push(entry_row(e, kind, busy));
    }
    if entries.len() > MAX_ROWS {
        list = list.push(muted_owned(format!(
            "… {} more (see git status)",
            entries.len() - MAX_ROWS
        )));
    }
    list.into()
}

fn entry_row<'a>(e: &'a Entry, kind: RowKind, busy: bool) -> Element<'a, Message> {
    let (letter, color, diff_kind) = match kind {
        RowKind::Conflict => ('!', theme::coral(), DiffKind::Unstaged),
        RowKind::Staged => (
            e.staged.map_or('?', Change::letter),
            theme::mint(),
            DiffKind::Staged,
        ),
        RowKind::Unstaged => (
            e.unstaged.map_or('?', Change::letter),
            theme::amber(),
            DiffKind::Unstaged,
        ),
        RowKind::Untracked => ('U', theme::violet(), DiffKind::Untracked),
    };
    let (dir, file) = match e.path.rsplit_once('/') {
        Some((d, f)) => (d, f),
        None => ("", e.path.as_str()),
    };
    let mut label = row![
        text(letter.to_string())
            .size(theme::size::SMALL)
            .font(TERMINAL_FONT)
            .color(color),
        text(file)
            .size(theme::size::SMALL)
            .color(theme::text())
            .wrapping(text::Wrapping::None),
    ]
    .spacing(theme::space::S)
    .align_y(iced::Alignment::Center);
    if !dir.is_empty() {
        label = label.push(
            text(dir)
                .size(theme::size::CAPTION)
                .color(theme::text_muted())
                .wrapping(text::Wrapping::None),
        );
    }
    let tip = match &e.orig_path {
        Some(orig) => format!("{orig} → {}\nClick to show the diff", e.path),
        None => format!("{}\nClick to show the diff", e.path),
    };
    // The tooltip belongs to the name only, not to the stage button next to it.
    let open = tooltip(
        mouse_area(container(label).width(Length::Fill).clip(true))
            .on_press(Message::GitShowDiff(e.path.clone(), diff_kind)),
        container(text(tip).size(theme::size::TINY).color(theme::text()))
            .padding(6)
            .style(|_| container::Style {
                background: Some(theme::elevated().into()),
                border: Border {
                    color: theme::border(),
                    width: 1.0,
                    radius: theme::radius::S.into(),
                },
                ..Default::default()
            }),
        tooltip::Position::Bottom,
    );
    let action = match kind {
        RowKind::Staged => Some(small_button(
            "−",
            (!busy).then(|| Message::GitUnstage(vec![e.path.clone()])),
            "Unstage",
        )),
        RowKind::Unstaged | RowKind::Untracked => Some(small_button(
            "+",
            (!busy).then(|| Message::GitStage(vec![e.path.clone()])),
            "Stage",
        )),
        // Staging a conflict marks it resolved; that is left to the terminal.
        RowKind::Conflict => None,
    };
    let mut r = row![open]
        .spacing(theme::space::XS)
        .align_y(iced::Alignment::Center);
    if let Some(a) = action {
        r = r.push(a);
    }
    container(r).padding([3, 4]).into()
}

fn commit_box<'a>(git: &'a GitState, status: &'a Status) -> Element<'a, Message> {
    let staged = status.staged().count();
    let editor = text_editor(&git.commit_message)
        .placeholder("Commit message")
        .on_action(Message::GitCommitEdit)
        .size(theme::size::SMALL)
        .padding([6, 8])
        .min_height(52.0)
        .max_height(140.0)
        .style(|_, status| {
            let focused = matches!(status, text_editor::Status::Focused { .. });
            text_editor::Style {
                background: theme::obsidian().into(),
                border: Border {
                    color: if focused {
                        theme::cyan()
                    } else {
                        theme::border()
                    },
                    width: 1.0,
                    radius: theme::radius::M.into(),
                },
                placeholder: theme::text_muted(),
                value: theme::text(),
                selection: theme::selection(),
            }
        });
    let label = if git.action_running {
        "Working…".to_owned()
    } else {
        format!("Commit {staged} staged file{}", plural(staged))
    };
    let hint = if status.conflicted().next().is_some() {
        "Resolve the conflicts first."
    } else if staged == 0 {
        "Stage files with + to commit them."
    } else {
        "Runs git commit (including the repository's hooks)."
    };
    column![
        text("COMMIT")
            .size(theme::size::TINY)
            .color(theme::text_muted()),
        editor,
        row![
            space::horizontal(),
            small_button(
                &label,
                git.can_commit()
                    .then_some(Message::Command(CommandId::GitCommit)),
                "git commit -m <message>"
            )
        ],
        muted(hint),
    ]
    .spacing(theme::space::S)
    .into()
}

fn log_block(log: Option<&Result<Vec<Commit>, String>>) -> Element<'_, Message> {
    let mut list = column![
        text("RECENT COMMITS")
            .size(theme::size::TINY)
            .color(theme::text_muted())
    ]
    .spacing(theme::space::XS);
    match log {
        None => list = list.push(muted("Reading…")),
        Some(Err(e)) => {
            list = list.push(
                text(e.as_str())
                    .size(theme::size::TINY)
                    .color(theme::coral()),
            )
        }
        Some(Ok(commits)) if commits.is_empty() => list = list.push(muted("No commits yet.")),
        Some(Ok(commits)) => {
            for c in commits {
                list = list.push(
                    column![
                        row![
                            text(c.hash.as_str())
                                .size(theme::size::TINY)
                                .font(TERMINAL_FONT)
                                .color(theme::cyan()),
                            text(c.subject.as_str())
                                .size(theme::size::SMALL)
                                .color(theme::text())
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(theme::space::S),
                        text(format!("{} · {}", c.author, c.when))
                            .size(theme::size::CAPTION)
                            .color(theme::text_muted()),
                    ]
                    .spacing(1),
                );
            }
        }
    }
    list.into()
}

/// Rows shown per branch list.
const MAX_BRANCHES: usize = 40;

fn branches_block(git: &GitState) -> Element<'_, Message> {
    let mut list = column![
        text("BRANCHES")
            .size(theme::size::TINY)
            .color(theme::text_muted())
    ]
    .spacing(3);
    let branches = match &git.branches {
        None => return list.push(muted("Reading…")).into(),
        Some(Err(e)) => {
            return list
                .push(
                    text(e.as_str())
                        .size(theme::size::TINY)
                        .color(theme::coral()),
                )
                .into();
        }
        Some(Ok(b)) => b,
    };
    let busy = git.action_running;
    if let Some(target) = &git.pending_switch {
        list = list.push(
            container(
                column![
                    text(format!("Switch to {target}?")).size(theme::size::SMALL).color(theme::text()),
                    muted("git switch keeps your uncommitted changes, or refuses if they would be overwritten."),
                    row![
                        space::horizontal(),
                        small_button("Cancel", Some(Message::GitSwitchCancel), "Stay on this branch"),
                        small_button(
                            "Switch",
                            (!busy).then_some(Message::GitSwitchConfirm),
                            "git switch"
                        ),
                    ]
                    .spacing(theme::space::S),
                ]
                .spacing(theme::space::XS),
            )
            .padding(6)
            .style(|_| container::Style {
                background: Some(theme::elevated().into()),
                border: Border {
                    color: theme::amber(),
                    width: 1.0,
                    radius: theme::radius::M.into(),
                },
                ..Default::default()
            }),
        );
    }
    let local: Vec<&BranchRef> = branches.iter().filter(|b| !b.remote).collect();
    for b in local.iter().take(MAX_BRANCHES) {
        let mut info = b.name.clone();
        if b.ahead > 0 {
            info.push_str(&format!(" ↑{}", b.ahead));
        }
        if b.behind > 0 {
            info.push_str(&format!(" ↓{}", b.behind));
        }
        if b.upstream_gone {
            info.push_str(" (upstream gone)");
        }
        let name = text(info)
            .size(theme::size::SMALL)
            .color(if b.current {
                theme::cyan()
            } else {
                theme::text()
            })
            .wrapping(text::Wrapping::None);
        let mut r = row![container(name).width(Length::Fill).clip(true)]
            .spacing(theme::space::XS)
            .align_y(iced::Alignment::Center);
        if b.current {
            r = r.push(
                text("current")
                    .size(theme::size::CAPTION)
                    .color(theme::text_muted()),
            );
        } else {
            r = r.push(small_button(
                "Switch",
                (!busy).then(|| Message::GitSwitchAsk(b.name.clone())),
                "Switch to this branch (asks first)",
            ));
        }
        list = list.push(r);
    }
    if local.len() > MAX_BRANCHES {
        list = list.push(muted_owned(format!(
            "… {} more",
            local.len() - MAX_BRANCHES
        )));
    }

    // New branch from HEAD.
    let name_ok = !git.new_branch.trim().is_empty() && !busy;
    let input = text_input("new-branch-name", &git.new_branch)
        .on_input(Message::GitNewBranchName)
        .on_submit_maybe(name_ok.then_some(Message::GitCreateBranch))
        .size(theme::size::SMALL)
        .padding([4, 6])
        .style(|_, status| text_input::Style {
            background: theme::obsidian().into(),
            border: Border {
                color: if matches!(status, text_input::Status::Focused { .. }) {
                    theme::cyan()
                } else {
                    theme::border()
                },
                width: 1.0,
                radius: theme::radius::S.into(),
            },
            icon: theme::text_muted(),
            placeholder: theme::text_muted(),
            value: theme::text(),
            selection: theme::selection(),
        });
    list = list.push(
        row![
            input,
            small_button(
                "Create",
                name_ok.then_some(Message::GitCreateBranch),
                "git switch -c <name> (from the current commit)"
            )
        ]
        .spacing(theme::space::XS)
        .align_y(iced::Alignment::Center),
    );

    // Remote branches, collapsed by default.
    let remote: Vec<&BranchRef> = branches.iter().filter(|b| b.remote).collect();
    if !remote.is_empty() {
        let label = format!(
            "{} Remote branches ({})",
            if git.show_remotes { "▾" } else { "▸" },
            remote.len()
        );
        list = list.push(
            mouse_area(
                text(label)
                    .size(theme::size::TINY)
                    .color(theme::text_muted()),
            )
            .on_press(Message::Command(CommandId::GitToggleRemoteBranches)),
        );
        if git.show_remotes {
            for b in remote.iter().take(MAX_BRANCHES) {
                // `origin/x` is already checked out as `x`: nothing to track.
                let local_name = b.name.split_once('/').map_or(b.name.as_str(), |(_, n)| n);
                let tracked = local.iter().any(|l| l.name == local_name);
                let mut r = row![
                    container(
                        text(b.name.as_str())
                            .size(theme::size::SMALL)
                            .color(theme::text())
                            .wrapping(text::Wrapping::None)
                    )
                    .width(Length::Fill)
                    .clip(true)
                ]
                .spacing(theme::space::XS)
                .align_y(iced::Alignment::Center);
                if !tracked {
                    r = r.push(small_button(
                        "Track",
                        (!busy).then(|| Message::GitTrack(b.name.clone())),
                        "git switch --track (creates a local branch and switches to it)",
                    ));
                }
                list = list.push(r);
            }
            if remote.len() > MAX_BRANCHES {
                list = list.push(muted_owned(format!(
                    "… {} more",
                    remote.len() - MAX_BRANCHES
                )));
            }
        }
    }
    list.into()
}

/// Height of the activity chart's plot area.
const CHART_HEIGHT: f32 = 72.0;

/// Commits per day as bars (one series, so no legend: the title names it).
pub fn activity_chart(activity: &Activity) -> Element<'_, Message> {
    let max = activity.days.iter().map(|(_, n)| *n).max().unwrap_or(0);
    let title = format!(
        "Commits per day · last {} days · {} total",
        activity.days.len(),
        activity.total
    );
    let mut bars = row![]
        .spacing(theme::space::XXS)
        .align_y(iced::Alignment::End)
        .height(CHART_HEIGHT);
    for (day, count) in &activity.days {
        let height = if *count == 0 || max == 0 {
            0.0
        } else {
            // At least 3 px so a single commit stays visible next to a busy day.
            (CHART_HEIGHT * *count as f32 / max as f32).max(3.0)
        };
        let bar = container(space::vertical().height(height))
            .width(Length::Fill)
            .height(height)
            .style(|_| container::Style {
                background: Some(theme::chart_cyan().into()),
                border: Border {
                    radius: iced::border::Radius {
                        top_left: 4.0,
                        top_right: 4.0,
                        bottom_right: 0.0,
                        bottom_left: 0.0,
                    },
                    ..Default::default()
                },
                ..Default::default()
            });
        let label = format!(
            "{}: {count} commit{}",
            day.format("%a %-d %b %Y"),
            if *count == 1 { "" } else { "s" }
        );
        // The hit target is the whole column, not just the (maybe tiny) bar.
        let column_area = container(bar)
            .width(Length::Fill)
            .height(CHART_HEIGHT)
            .align_y(iced::alignment::Vertical::Bottom);
        bars = bars.push(
            container(tooltip(
                column_area,
                container(text(label).size(theme::size::TINY).color(theme::text()))
                    .padding(5)
                    .style(|_| container::Style {
                        background: Some(theme::elevated().into()),
                        border: Border {
                            color: theme::border(),
                            width: 1.0,
                            radius: theme::radius::S.into(),
                        },
                        ..Default::default()
                    }),
                tooltip::Position::Top,
            ))
            .width(Length::Fill),
        );
    }
    let baseline = container(space::vertical().height(1))
        .width(Length::Fill)
        .height(1)
        .style(|_| container::Style {
            background: Some(theme::border().into()),
            ..Default::default()
        });
    let first = activity
        .days
        .first()
        .map(|(d, _)| d.format("%-d %b").to_string());
    let axis = row![
        text(first.unwrap_or_default())
            .size(theme::size::CAPTION)
            .color(theme::text_muted()),
        space::horizontal(),
        text("today")
            .size(theme::size::CAPTION)
            .color(theme::text_muted()),
    ];
    column![
        row![
            text(title).size(theme::size::SMALL).color(theme::text()),
            space::horizontal(),
            text(if max > 0 {
                format!("max {max}/day")
            } else {
                String::new()
            })
            .size(theme::size::CAPTION)
            .color(theme::text_muted()),
        ],
        bars,
        baseline,
        axis,
    ]
    .spacing(theme::space::XS)
    .into()
}

/// The history overlay (Esc closes).
pub fn view_history<'a>(h: &'a HistoryView, branch: String) -> Element<'a, Message> {
    let header = row![
        text(format!("History · {branch}"))
            .size(theme::size::LARGE)
            .color(theme::text()),
        space::horizontal(),
        small_button("Close (Esc)", Some(Message::GitHistoryClose), "Close"),
    ]
    .align_y(iced::Alignment::Center);

    let chart: Element<'_, Message> = match &h.activity {
        None => muted("Counting commits…"),
        Some(Err(e)) => text(e.as_str())
            .size(theme::size::TINY)
            .color(theme::coral())
            .into(),
        Some(Ok(a)) => activity_chart(a),
    };

    let list: Element<'_, Message> = match &h.entries {
        None => muted("Reading history…"),
        Some(Err(e)) => text(e.as_str())
            .size(theme::size::SMALL)
            .color(theme::coral())
            .into(),
        Some(Ok(entries)) if entries.is_empty() => muted("No commits yet."),
        Some(Ok(entries)) => {
            let mut col = column![].spacing(theme::space::XXS);
            for e in entries {
                let selected = h.selected.as_deref() == Some(e.hash.as_str());
                let mut top = row![
                    text(e.short.as_str())
                        .size(theme::size::TINY)
                        .font(TERMINAL_FONT)
                        .color(theme::cyan()),
                    text(e.subject.as_str())
                        .size(theme::size::SMALL)
                        .color(theme::text())
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(theme::space::S);
                if !e.refs.is_empty() {
                    top = top.push(
                        text(compact_refs(&e.refs))
                            .size(theme::size::CAPTION)
                            .color(theme::amber())
                            .wrapping(text::Wrapping::None),
                    );
                }
                let entry = column![
                    container(top).clip(true),
                    text(format!("{} · {}", e.author, e.when))
                        .size(theme::size::CAPTION)
                        .color(theme::text_muted()),
                ]
                .spacing(1);
                col = col.push(
                    mouse_area(container(entry).padding([5, 8]).width(Length::Fill).style(
                        move |_| container::Style {
                            background: selected.then(|| theme::elevated().into()),
                            border: Border {
                                color: if selected {
                                    theme::cyan()
                                } else {
                                    Color::TRANSPARENT
                                },
                                width: if selected { 1.0 } else { 0.0 },
                                radius: 5.0.into(),
                            },
                            ..Default::default()
                        },
                    ))
                    .on_press(Message::GitSelectCommit(e.hash.clone())),
                );
            }
            scrollable(col).height(Length::Fill).into()
        }
    };

    let detail: Element<'_, Message> = match (&h.selected, &h.detail) {
        (None, _) => muted("Select a commit to see its changes."),
        (Some(_), None) => muted("Loading…"),
        (Some(_), Some(Err(e))) => text(e.as_str())
            .size(theme::size::SMALL)
            .color(theme::coral())
            .into(),
        (Some(_), Some(Ok(d))) => diff_lines(d),
    };

    let panel = |content: Element<'a, Message>, width: Length| {
        container(content)
            .padding(8)
            .width(width)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(theme::obsidian().into()),
                border: Border {
                    color: theme::border(),
                    width: 1.0,
                    radius: theme::radius::M.into(),
                },
                ..Default::default()
            })
    };
    let card = container(
        column![
            header,
            container(chart)
                .padding(10)
                .width(Length::Fill)
                .style(|_| container::Style {
                    background: Some(theme::graphite().into()),
                    border: Border {
                        color: theme::border(),
                        width: 1.0,
                        radius: theme::radius::M.into(),
                    },
                    ..Default::default()
                }),
            row![
                panel(list, Length::FillPortion(2)),
                panel(detail, Length::FillPortion(3))
            ]
            .spacing(theme::space::L)
            .height(Length::Fill),
        ]
        .spacing(theme::space::L),
    );
    overlay(card, Message::GitHistoryClose)
}

/// `HEAD -> main, origin/main, tag: v1, x, y` → `HEAD -> main, origin/main, tag: v1 +2`
/// (a busy commit would otherwise fill several lines).
fn compact_refs(refs: &str) -> String {
    const SHOWN: usize = 3;
    let parts: Vec<&str> = refs.split(", ").filter(|p| !p.is_empty()).collect();
    let mut s = parts
        .iter()
        .take(SHOWN)
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    if parts.len() > SHOWN {
        s.push_str(&format!(" +{}", parts.len() - SHOWN));
    }
    s
}

/// Card + dimmed backdrop shared by the overlays; clicks outside close.
fn overlay<'a>(card: container::Container<'a, Message>, close: Message) -> Element<'a, Message> {
    let card = card
        .padding(14)
        .width(Length::Fill)
        .height(Length::Fill)
        .max_width(1100.0)
        .style(|_| container::Style {
            background: Some(theme::surface().into()),
            border: Border {
                color: theme::border(),
                width: 1.0,
                radius: theme::radius::XL.into(),
            },
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.55),
                offset: Vector::new(0.0, 12.0),
                blur_radius: 32.0,
            },
            ..Default::default()
        });
    let backdrop = container(opaque(card))
        .center(Length::Fill)
        .padding(28)
        .style(|_| container::Style {
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.35).into()),
            ..Default::default()
        });
    opaque(mouse_area(backdrop).on_press(close))
}

/// Colored diff / `git show` lines in a scrollable area.
fn diff_lines(d: &Diff) -> Element<'_, Message> {
    if d.lines.is_empty() {
        return muted("No differences (binary, or only mode changes).");
    }
    let mut lines = column![].spacing(0);
    for line in &d.lines {
        lines = lines.push(
            text(line.as_str())
                .font(TERMINAL_FONT)
                .size(theme::size::SMALL)
                .color(diff_color(line))
                .wrapping(text::Wrapping::None),
        );
    }
    if d.truncated {
        lines = lines.push(muted_owned(format!(
            "… cut off after {} lines",
            forgeterm_git::MAX_DIFF_LINES
        )));
    }
    scrollable(lines)
        .direction(scrollable::Direction::Both {
            vertical: scrollable::Scrollbar::default(),
            horizontal: scrollable::Scrollbar::default(),
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// The diff overlay (Esc closes).
pub fn view_diff(diff: &DiffView) -> Element<'_, Message> {
    let kind = match diff.kind {
        DiffKind::Unstaged => "unstaged changes",
        DiffKind::Staged => "staged changes",
        DiffKind::Untracked => "new file",
    };
    let header = row![
        text(diff.path.as_str())
            .size(theme::size::LARGE)
            .color(theme::text()),
        text(kind)
            .size(theme::size::TINY)
            .color(theme::text_muted()),
        space::horizontal(),
        small_button(
            "Open in viewer",
            Some(Message::GitOpenInViewer(diff.path.clone())),
            "Show the whole file in the code viewer"
        ),
        small_button("Close (Esc)", Some(Message::GitDiffClose), "Close"),
    ]
    .spacing(theme::space::L)
    .align_y(iced::Alignment::Center);

    let content: Element<'_, Message> = match &diff.result {
        None => muted("Loading…"),
        Some(Err(e)) => text(e.as_str())
            .size(theme::size::SMALL)
            .color(theme::coral())
            .into(),
        Some(Ok(d)) if d.lines.is_empty() => {
            muted("No differences (binary, or only mode changes).")
        }
        Some(Ok(d)) => {
            let mut lines = column![].spacing(0);
            for line in &d.lines {
                lines = lines.push(
                    text(line.as_str())
                        .font(TERMINAL_FONT)
                        .size(theme::size::SMALL)
                        .color(diff_color(line))
                        .wrapping(text::Wrapping::None),
                );
            }
            if d.truncated {
                lines = lines.push(muted_owned(format!(
                    "… cut off after {} lines",
                    forgeterm_git::MAX_DIFF_LINES
                )));
            }
            scrollable(lines)
                .direction(scrollable::Direction::Both {
                    vertical: scrollable::Scrollbar::default(),
                    horizontal: scrollable::Scrollbar::default(),
                })
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }
    };

    let card = container(
        column![
            header,
            container(content)
                .padding(10)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(theme::obsidian().into()),
                    border: Border {
                        color: theme::border(),
                        width: 1.0,
                        radius: theme::radius::M.into(),
                    },
                    ..Default::default()
                }),
        ]
        .spacing(theme::space::L),
    )
    .padding(14)
    .width(Length::Fill)
    .height(Length::Fill)
    .max_width(1100.0)
    .style(|_| container::Style {
        background: Some(theme::surface().into()),
        border: Border {
            color: theme::border(),
            width: 1.0,
            radius: theme::radius::XL.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.55),
            offset: Vector::new(0.0, 12.0),
            blur_radius: 32.0,
        },
        ..Default::default()
    });
    let backdrop = container(opaque(card))
        .center(Length::Fill)
        .padding(28)
        .style(|_| container::Style {
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.35).into()),
            ..Default::default()
        });
    opaque(mouse_area(backdrop).on_press(Message::GitDiffClose))
}

fn diff_color(line: &str) -> Color {
    if line.starts_with("+++") || line.starts_with("---") {
        theme::text_muted()
    } else if line.starts_with('+') {
        theme::mint()
    } else if line.starts_with('-') {
        theme::coral()
    } else if line.starts_with("@@") {
        theme::cyan()
    } else if line.starts_with("diff ")
        || line.starts_with("index ")
        || line.starts_with("new file")
    {
        theme::text_muted()
    } else {
        theme::text()
    }
}

fn muted<'a>(s: &'a str) -> Element<'a, Message> {
    text(s)
        .size(theme::size::TINY)
        .color(theme::text_muted())
        .into()
}

fn muted_owned<'a>(s: String) -> Element<'a, Message> {
    text(s)
        .size(theme::size::TINY)
        .color(theme::text_muted())
        .into()
}

fn path_text<'a>(s: String) -> Element<'a, Message> {
    text(s)
        .size(theme::size::TINY)
        .color(theme::text_muted())
        .wrapping(text::Wrapping::Glyph)
        .into()
}

fn small_button<'a>(label: &str, on_press: Option<Message>, tip: &'a str) -> Element<'a, Message> {
    let enabled = on_press.is_some();
    let b = button(text(label.to_owned()).size(theme::size::TINY))
        .on_press_maybe(on_press)
        .padding([3, 8])
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(
                    if hovered {
                        theme::elevated()
                    } else {
                        theme::surface()
                    }
                    .into(),
                ),
                text_color: if !enabled {
                    theme::disabled()
                } else if hovered {
                    theme::cyan()
                } else {
                    theme::text()
                },
                border: Border {
                    color: theme::border(),
                    width: 1.0,
                    radius: theme::radius::S.into(),
                },
                ..Default::default()
            }
        });
    tooltip(
        b,
        container(text(tip).size(theme::size::TINY).color(theme::text()))
            .padding(5)
            .style(|_| container::Style {
                background: Some(theme::elevated().into()),
                ..Default::default()
            }),
        tooltip::Position::Bottom,
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(raw: &str) -> GitState {
        let mut g = GitState::default();
        g.apply(Snapshot {
            dir: PathBuf::from("/r"),
            repo: Repo::Found {
                root: PathBuf::from("/r"),
                status: forgeterm_git::parse_status(raw.as_bytes()),
            },
            log: None,
            branches: None,
        });
        g
    }

    #[test]
    fn summary_reflects_branch_and_changes() {
        let clean = state("# branch.oid abc\0# branch.head main\0");
        assert_eq!(clean.summary().unwrap().0, "main · clean");
        let dirty =
            state("# branch.oid abc\0# branch.head dev\0# branch.ab +2 -0\0? a.txt\0? b.txt\0");
        assert_eq!(dirty.summary().unwrap().0, "dev ↑2 · 2 changed");
        let conflict =
            state("# branch.oid abc\0# branch.head main\0u UU N... 1 1 1 1 h h h x.rs\0");
        assert_eq!(conflict.summary().unwrap().0, "main · 1 conflict");
        assert!(GitState::default().summary().is_none());
    }

    #[test]
    fn long_ref_lists_are_shortened() {
        assert_eq!(compact_refs("HEAD -> main"), "HEAD -> main");
        assert_eq!(
            compact_refs("HEAD -> a, origin/a, tag: v1, b, c"),
            "HEAD -> a, origin/a, tag: v1 +2"
        );
        assert_eq!(compact_refs(""), "");
    }

    #[test]
    fn action_message_is_cleared_when_the_repository_changes() {
        let mut g = state("# branch.oid abc\0# branch.head main\0");
        g.last_action = Some(Ok("Committed abc".into()));
        // Same repository (another tick): the message stays.
        g.apply(Snapshot {
            dir: PathBuf::from("/r/sub"),
            repo: g.repo.clone().unwrap(),
            log: None,
            branches: None,
        });
        assert!(g.last_action.is_some());
        // Other repository: gone.
        g.apply(Snapshot {
            dir: PathBuf::from("/other"),
            repo: Repo::Found {
                root: PathBuf::from("/other"),
                status: Status::default(),
            },
            log: None,
            branches: None,
        });
        assert!(g.last_action.is_none());
    }

    #[test]
    fn commit_needs_staged_files_message_and_no_conflicts() {
        let mut g = state("# branch.oid abc\0# branch.head main\0? a.txt\0");
        let type_text = |g: &mut GitState, s: &str| {
            for c in s.chars() {
                g.commit_message
                    .perform(text_editor::Action::Edit(text_editor::Edit::Insert(c)));
            }
        };
        type_text(&mut g, "msg");
        assert!(!g.can_commit(), "nothing staged");
        let mut g = state("# branch.oid abc\0# branch.head main\x001 A. N... 0 1 1 0 h new.rs\0");
        assert!(!g.can_commit(), "no message");
        type_text(&mut g, "   ");
        assert!(!g.can_commit(), "blank message");
        type_text(&mut g, "Add new.rs");
        assert!(g.can_commit());
        g.action_running = true;
        assert!(!g.can_commit(), "busy");
    }
}
