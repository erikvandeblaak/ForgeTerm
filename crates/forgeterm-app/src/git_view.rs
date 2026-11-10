//! Git side panel (status, stage/unstage, commit, recent commits), the diff overlay
//! and the status-bar summary. All git work happens in `forgeterm-git` on background
//! threads; this module only holds what came back and draws it.

use std::path::PathBuf;

use forgeterm_git::{Change, Commit, Diff, DiffKind, Entry, Status};
use iced::widget::{
    button, column, container, mouse_area, opaque, row, scrollable, space, text, text_editor,
    tooltip,
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
    /// Outcome of the last action, shown until the next one.
    pub last_action: Option<Result<String, String>>,
    pub commit_message: text_editor::Content,
    pub diff: Option<DiffView>,
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
        }
        self.dir = Some(snapshot.dir);
        self.repo = Some(snapshot.repo);
        if snapshot.log.is_some() {
            self.log = snapshot.log;
        }
        if self.root().is_none() {
            self.log = None;
        }
    }

    /// Status-bar text: `main ↑1 ↓2 · 3 changed`; `None` outside a repository.
    pub fn summary(&self) -> Option<(String, Color)> {
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
            theme::CORAL
        } else if status.change_count() > 0 {
            s.push_str(&format!(" · {} changed", status.change_count()));
            theme::AMBER
        } else {
            s.push_str(" · clean");
            theme::MINT
        };
        Some((s, color))
    }

    fn can_commit(&self) -> bool {
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
        text("GIT").size(11).color(theme::TEXT_MUTED),
        space::horizontal(),
        small_button(
            "↻",
            Some(Message::Command(CommandId::GitRefresh)),
            "Refresh"
        ),
    ]
    .align_y(iced::Alignment::Center);

    let mut body = column![].spacing(10);
    match &git.repo {
        None => body = body.push(muted("Reading…")),
        Some(Repo::Outside) => {
            body = body.push(muted("Not a Git repository:"));
            if let Some(dir) = &git.dir {
                body = body.push(path_text(dir.display().to_string()));
            }
        }
        Some(Repo::Error(e)) => {
            body = body.push(text(e.as_str()).size(12).color(theme::CORAL));
        }
        Some(Repo::Found { root, status }) => {
            body = body.push(branch_block(root, status));
            if let Some(result) = &git.last_action {
                body = body.push(match result {
                    Ok(m) => text(m.as_str()).size(12).color(theme::MINT),
                    Err(e) => text(e.as_str()).size(12).color(theme::CORAL),
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
            body = body.push(log_block(git.log.as_ref()));
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
        .spacing(8),
    )
    .padding([10, 10])
    .width(Length::Fixed(WIDTH))
    .height(Length::Fill)
    .style(|_| container::Style {
        background: Some(theme::GRAPHITE.into()),
        border: Border {
            color: theme::BORDER,
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
            text(b.label()).size(14).color(theme::CYAN),
            space::horizontal(),
            text(name).size(11).color(theme::TEXT_MUTED),
        ]
        .align_y(iced::Alignment::Center),
        text(upstream).size(11).color(theme::TEXT_MUTED),
    ]
    .spacing(2)
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
            .size(11)
            .color(if kind == RowKind::Conflict {
                theme::CORAL
            } else {
                theme::TEXT_MUTED
            }),
        space::horizontal(),
    ]
    .align_y(iced::Alignment::Center);
    if let Some((label, msg, tip)) = all_action {
        head = head.push(small_button(label, (!busy).then_some(msg), tip));
    }
    let mut list = column![head].spacing(2);
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
        RowKind::Conflict => ('!', theme::CORAL, DiffKind::Unstaged),
        RowKind::Staged => (
            e.staged.map_or('?', Change::letter),
            theme::MINT,
            DiffKind::Staged,
        ),
        RowKind::Unstaged => (
            e.unstaged.map_or('?', Change::letter),
            theme::AMBER,
            DiffKind::Unstaged,
        ),
        RowKind::Untracked => ('U', theme::VIOLET, DiffKind::Untracked),
    };
    let (dir, file) = match e.path.rsplit_once('/') {
        Some((d, f)) => (d, f),
        None => ("", e.path.as_str()),
    };
    let mut label = row![
        text(letter.to_string())
            .size(12)
            .font(TERMINAL_FONT)
            .color(color),
        text(file)
            .size(12)
            .color(theme::TEXT)
            .wrapping(text::Wrapping::None),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);
    if !dir.is_empty() {
        label = label.push(
            text(dir)
                .size(10)
                .color(theme::TEXT_MUTED)
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
        container(text(tip).size(11).color(theme::TEXT))
            .padding(6)
            .style(|_| container::Style {
                background: Some(theme::ELEVATED.into()),
                border: Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
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
    let mut r = row![open].spacing(4).align_y(iced::Alignment::Center);
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
        .size(12)
        .padding([6, 8])
        .min_height(52.0)
        .max_height(140.0)
        .style(|_, status| {
            let focused = matches!(status, text_editor::Status::Focused { .. });
            text_editor::Style {
                background: theme::OBSIDIAN.into(),
                border: Border {
                    color: if focused { theme::CYAN } else { theme::BORDER },
                    width: 1.0,
                    radius: 6.0.into(),
                },
                placeholder: theme::TEXT_MUTED,
                value: theme::TEXT,
                selection: theme::SELECTION,
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
        text("COMMIT").size(11).color(theme::TEXT_MUTED),
        editor,
        row![
            space::horizontal(),
            small_button(
                &label,
                git.can_commit().then_some(Message::GitCommit),
                "git commit -m <message>"
            )
        ],
        muted(hint),
    ]
    .spacing(6)
    .into()
}

fn log_block(log: Option<&Result<Vec<Commit>, String>>) -> Element<'_, Message> {
    let mut list = column![text("RECENT COMMITS").size(11).color(theme::TEXT_MUTED)].spacing(4);
    match log {
        None => list = list.push(muted("Reading…")),
        Some(Err(e)) => list = list.push(text(e.as_str()).size(11).color(theme::CORAL)),
        Some(Ok(commits)) if commits.is_empty() => list = list.push(muted("No commits yet.")),
        Some(Ok(commits)) => {
            for c in commits {
                list = list.push(
                    column![
                        row![
                            text(c.hash.as_str())
                                .size(11)
                                .font(TERMINAL_FONT)
                                .color(theme::CYAN),
                            text(c.subject.as_str())
                                .size(12)
                                .color(theme::TEXT)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(6),
                        text(format!("{} · {}", c.author, c.when))
                            .size(10)
                            .color(theme::TEXT_MUTED),
                    ]
                    .spacing(1),
                );
            }
        }
    }
    list.into()
}

/// The diff overlay (Esc closes).
pub fn view_diff(diff: &DiffView) -> Element<'_, Message> {
    let kind = match diff.kind {
        DiffKind::Unstaged => "unstaged changes",
        DiffKind::Staged => "staged changes",
        DiffKind::Untracked => "new file",
    };
    let header = row![
        text(diff.path.as_str()).size(14).color(theme::TEXT),
        text(kind).size(11).color(theme::TEXT_MUTED),
        space::horizontal(),
        small_button("Close (Esc)", Some(Message::GitDiffClose), "Close"),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center);

    let content: Element<'_, Message> = match &diff.result {
        None => muted("Loading…"),
        Some(Err(e)) => text(e.as_str()).size(12).color(theme::CORAL).into(),
        Some(Ok(d)) if d.lines.is_empty() => {
            muted("No differences (binary, or only mode changes).")
        }
        Some(Ok(d)) => {
            let mut lines = column![].spacing(0);
            for line in &d.lines {
                lines = lines.push(
                    text(line.as_str())
                        .font(TERMINAL_FONT)
                        .size(12)
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
                    background: Some(theme::OBSIDIAN.into()),
                    border: Border {
                        color: theme::BORDER,
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    ..Default::default()
                }),
        ]
        .spacing(10),
    )
    .padding(14)
    .width(Length::Fill)
    .height(Length::Fill)
    .max_width(1100.0)
    .style(|_| container::Style {
        background: Some(theme::SURFACE.into()),
        border: Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 10.0.into(),
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
        theme::TEXT_MUTED
    } else if line.starts_with('+') {
        theme::MINT
    } else if line.starts_with('-') {
        theme::CORAL
    } else if line.starts_with("@@") {
        theme::CYAN
    } else if line.starts_with("diff ")
        || line.starts_with("index ")
        || line.starts_with("new file")
    {
        theme::TEXT_MUTED
    } else {
        theme::TEXT
    }
}

fn muted<'a>(s: &'a str) -> Element<'a, Message> {
    text(s).size(11).color(theme::TEXT_MUTED).into()
}

fn muted_owned<'a>(s: String) -> Element<'a, Message> {
    text(s).size(11).color(theme::TEXT_MUTED).into()
}

fn path_text<'a>(s: String) -> Element<'a, Message> {
    text(s)
        .size(11)
        .color(theme::TEXT_MUTED)
        .wrapping(text::Wrapping::Glyph)
        .into()
}

fn small_button<'a>(label: &str, on_press: Option<Message>, tip: &'a str) -> Element<'a, Message> {
    let enabled = on_press.is_some();
    let b = button(text(label.to_owned()).size(11))
        .on_press_maybe(on_press)
        .padding([3, 8])
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(
                    if hovered {
                        theme::ELEVATED
                    } else {
                        theme::SURFACE
                    }
                    .into(),
                ),
                text_color: if !enabled {
                    theme::DISABLED
                } else if hovered {
                    theme::CYAN
                } else {
                    theme::TEXT
                },
                border: Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        });
    tooltip(
        b,
        container(text(tip).size(11).color(theme::TEXT))
            .padding(5)
            .style(|_| container::Style {
                background: Some(theme::ELEVATED.into()),
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
    fn action_message_is_cleared_when_the_repository_changes() {
        let mut g = state("# branch.oid abc\0# branch.head main\0");
        g.last_action = Some(Ok("Committed abc".into()));
        // Same repository (another tick): the message stays.
        g.apply(Snapshot {
            dir: PathBuf::from("/r/sub"),
            repo: g.repo.clone().unwrap(),
            log: None,
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
