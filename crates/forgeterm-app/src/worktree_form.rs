//! "Agent in a new worktree": choose an agent and a branch; ForgeTerm creates the
//! worktree beside the repository (`git worktree add -b`) and starts the agent
//! there in its own tab. Also the confirmation for removing a worktree.

use std::path::{Path, PathBuf};

use forgeterm_agents::AgentKind;
use iced::widget::{column, container, opaque, row, space, stack, text, text_input};
use iced::{Border, Color, Element, Length};

use crate::ai_view::AgentState;
use crate::app::Message;
use crate::mission_view::{action_button, agent_badge, tail};
use crate::theme;

pub const BRANCH_INPUT_ID: &str = "worktree-branch";

#[derive(Debug, Clone)]
pub struct WorktreeForm {
    pub root: PathBuf,
    pub kind: AgentKind,
    pub branch: String,
    /// Branch names known so far (suggestions skip them); read again on open.
    pub existing: Vec<String>,
    pub error: Option<String>,
    /// `git worktree add` is running.
    pub busy: bool,
}

/// `agent/claude-1`, `agent/claude-2`, …: the first one not in `existing`.
pub fn suggest_branch(kind: AgentKind, existing: &[String]) -> String {
    let base = match kind {
        AgentKind::ClaudeCode => "agent/claude",
        AgentKind::Codex => "agent/codex",
    };
    (1..)
        .map(|n| format!("{base}-{n}"))
        .find(|b| !existing.iter().any(|e| e == b))
        .unwrap_or_else(|| base.to_owned())
}

impl WorktreeForm {
    /// The repository's branches were read: a suggestion the user did not change
    /// skips them too. Returns whether the branch field changed.
    pub fn set_existing(&mut self, names: Vec<String>) -> bool {
        let untouched = self.branch == suggest_branch(self.kind, &self.existing);
        self.existing = names;
        let fresh = suggest_branch(self.kind, &self.existing);
        let changed = untouched && !self.busy && self.branch != fresh;
        if changed {
            self.branch = fresh;
        }
        changed
    }
}

/// Whether the user kept the suggestion (then switching agent updates it).
pub fn is_suggestion(branch: &str) -> bool {
    let b = branch.trim();
    ["agent/claude-", "agent/codex-"].iter().any(|p| {
        b.strip_prefix(p)
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    })
}

pub fn view<'a>(
    form: &'a WorktreeForm,
    agents: &'a [(AgentKind, AgentState)],
) -> Element<'a, Message> {
    let mut choices = row![].spacing(8);
    for (kind, state) in agents {
        let found = matches!(state, AgentState::Found(_));
        let selected = form.kind == *kind;
        let label = if found {
            kind.name().to_owned()
        } else {
            format!("{} (not installed)", kind.name())
        };
        choices = choices.push(
            iced::widget::button(
                row![agent_badge(*kind), text(label).size(13)]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
            )
            .padding([6, 10])
            .on_press_maybe((found && !form.busy).then_some(Message::WorktreeFormAgent(*kind)))
            .style(move |_, status| {
                let hovered = matches!(
                    status,
                    iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
                );
                iced::widget::button::Style {
                    background: Some(
                        if selected {
                            Color {
                                a: 0.16,
                                ..theme::CYAN
                            }
                        } else if hovered && found {
                            theme::ELEVATED
                        } else {
                            theme::GRAPHITE
                        }
                        .into(),
                    ),
                    text_color: if found { theme::TEXT } else { theme::DISABLED },
                    border: Border {
                        color: if selected { theme::CYAN } else { theme::BORDER },
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    ..Default::default()
                }
            }),
        );
    }

    let path = forgeterm_git::default_worktree_path(&form.root, &form.branch);
    let path_line = match &path {
        Some(p) => text(tail(&p.display().to_string(), 80))
            .size(12)
            .font(crate::terminal_view::TERMINAL_FONT)
            .color(theme::TEXT_MUTED),
        None => text("Type a branch name").size(12).color(theme::TEXT_MUTED),
    };
    let input = text_input("agent/claude-1", &form.branch)
        .id(BRANCH_INPUT_ID)
        .on_input_maybe((!form.busy).then_some(Message::WorktreeFormBranch))
        .on_submit(Message::WorktreeFormSubmit)
        .padding([6, 8])
        .size(14);

    let mut card = column![
        text("Agent in a new worktree").size(18).color(theme::TEXT),
        text(format!(
            "A new branch from the current commit of {}, checked out in its own folder, so the agent works without touching your working tree.",
            form.root.file_name().map_or_else(|| form.root.display().to_string(), |n| n.to_string_lossy().into_owned())
        ))
        .size(12)
        .color(theme::TEXT_MUTED),
        label("AGENT"),
        choices,
        label("NEW BRANCH"),
        input,
        label("FOLDER (beside the repository)"),
        path_line,
    ]
    .spacing(8);
    if let Some(e) = &form.error {
        card = card.push(text(e.clone()).size(12).color(theme::CORAL));
    }
    card = card.push(
        text("Runs git worktree add -b, including the repository's checkout hook, like the git command. Nothing is forced.")
            .size(11)
            .color(theme::DISABLED),
    );
    let can = !form.busy && path.is_some();
    card = card.push(
        row![
            space::horizontal(),
            action_button(
                "Cancel (Esc)".to_owned(),
                Some(Message::WorktreeFormCancel),
                theme::CYAN,
                false
            ),
            action_button(
                if form.busy {
                    "Creating…".to_owned()
                } else {
                    "Create and start (Enter)".to_owned()
                },
                can.then_some(Message::WorktreeFormSubmit),
                theme::CYAN,
                true,
            ),
        ]
        .spacing(8),
    );
    modal(card, Message::WorktreeFormCancel)
}

/// Confirmation before `git worktree remove` (never forced).
pub fn view_remove<'a>(path: &'a Path, branch: Option<&'a str>) -> Element<'a, Message> {
    let card = column![
        text("Remove this worktree?").size(18).color(theme::TEXT),
        text(path.display().to_string())
            .size(12)
            .font(crate::terminal_view::TERMINAL_FONT)
            .color(theme::TEXT_MUTED),
        text(format!(
            "The folder is deleted. The branch{} is kept, with all its commits. Git refuses if the worktree has uncommitted changes or untracked files; nothing is forced.",
            branch.map_or_else(String::new, |b| format!(" {b}"))
        ))
        .size(12)
        .color(theme::TEXT),
        row![
            space::horizontal(),
            action_button("Cancel (Esc)".to_owned(), Some(Message::WorktreeRemoveCancel), theme::CYAN, false),
            action_button("Remove (Enter)".to_owned(), Some(Message::WorktreeRemoveConfirm), theme::CORAL, true),
        ]
        .spacing(8),
    ]
    .spacing(10);
    modal(card, Message::WorktreeRemoveCancel)
}

fn label<'a>(s: &'a str) -> Element<'a, Message> {
    text(s).size(10).color(theme::TEXT_MUTED).into()
}

/// Centered card over a dimmed page; a click beside it cancels.
fn modal<'a>(card: impl Into<Element<'a, Message>>, cancel: Message) -> Element<'a, Message> {
    let card = container(card)
        .padding(22)
        .max_width(620)
        .style(|_| container::Style {
            background: Some(theme::SURFACE.into()),
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 12.0.into(),
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.55),
                offset: iced::Vector::new(0.0, 8.0),
                blur_radius: 28.0,
            },
            ..Default::default()
        });
    let backdrop = iced::widget::mouse_area(
        container(space::vertical())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.35).into()),
                ..Default::default()
            }),
    )
    .on_press(cancel);
    // `opaque`: the page below gets no hover either (no tooltips through the dimming).
    stack![
        opaque(backdrop),
        container(opaque(card)).center(Length::Fill)
    ]
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_skip_existing_branches() {
        let existing = vec![
            "agent/claude-1".to_owned(),
            "agent/claude-2".to_owned(),
            "main".to_owned(),
        ];
        assert_eq!(
            suggest_branch(AgentKind::ClaudeCode, &existing),
            "agent/claude-3"
        );
        assert_eq!(suggest_branch(AgentKind::Codex, &existing), "agent/codex-1");
        assert!(is_suggestion("agent/claude-3"));
        assert!(is_suggestion(" agent/codex-12 "));
        assert!(!is_suggestion("agent/claude-"));
        assert!(!is_suggestion("feature/login"));
        assert!(!is_suggestion("agent/claude-1x"));
    }

    #[test]
    fn late_branch_list_updates_only_an_untouched_suggestion() {
        let form = |branch: &str| WorktreeForm {
            root: PathBuf::from("/work/shop"),
            kind: AgentKind::ClaudeCode,
            branch: branch.to_owned(),
            existing: Vec::new(),
            error: None,
            busy: false,
        };
        let names = vec!["main".to_owned(), "agent/claude-1".to_owned()];
        let mut f = form("agent/claude-1");
        assert!(f.set_existing(names.clone()));
        assert_eq!(f.branch, "agent/claude-2");
        // Typed by the user: kept, even when it looks like a suggestion.
        let mut f = form("agent/claude-7");
        assert!(!f.set_existing(names.clone()));
        assert_eq!(f.branch, "agent/claude-7");
        let mut f = form("feature/login");
        assert!(!f.set_existing(names.clone()));
        assert_eq!(f.existing, names);
    }
}
