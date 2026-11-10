//! AI workspace side panel: detected agents and the project's agent files.

use forgeterm_agents::{Agent, AgentKind, ContextItem, ProjectContext};
use iced::widget::{button, column, container, mouse_area, row, scrollable, space, text, tooltip};
use iced::{Border, Color, Element, Length};

use crate::app::Message;
use crate::commands::CommandId;
use crate::theme;

pub const WIDTH: f32 = 290.0;

/// Detection state of one agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentState {
    Detecting,
    Missing,
    Found(Agent),
}

pub fn view<'a>(
    agents: &'a [(AgentKind, AgentState)],
    context: Option<&'a ProjectContext>,
) -> Element<'a, Message> {
    let header = row![
        text("AI WORKSPACE").size(11).color(theme::TEXT_MUTED),
        space::horizontal(),
        small_button("↻", Some(Message::Command(CommandId::AiRefresh)), false),
    ]
    .align_y(iced::Alignment::Center);

    let mut body = column![].spacing(10);
    body = body.push(section("AGENTS"));
    for (kind, state) in agents {
        body = body.push(agent_card(*kind, state));
    }

    body = body.push(section("PROJECT"));
    match context {
        None => body = body.push(muted("Scanning…")),
        Some(ctx) => {
            let name = ctx.root.file_name().map_or_else(
                || ctx.root.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            body = body.push(
                column![
                    text(name).size(13).color(theme::TEXT),
                    text(ctx.root.display().to_string())
                        .size(11)
                        .color(theme::TEXT_MUTED)
                        .wrapping(text::Wrapping::None),
                    muted(if ctx.is_git {
                        "Git repository root"
                    } else {
                        "No Git repository found; using this folder"
                    }),
                ]
                .spacing(1),
            );
            if ctx.items.is_empty() {
                body = body.push(muted(
                    "No agent files (CLAUDE.md, AGENTS.md, .claude/…) in this project.",
                ));
            } else {
                body = body.push(muted(
                    "Click to insert into the focused terminal (no Enter).",
                ));
                for item in &ctx.items {
                    body = body.push(context_row(item));
                }
            }
        }
    }

    container(
        column![
            container(header).padding([8, 10]),
            scrollable(container(body).padding([0, 10]))
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new().width(6).scroller_width(6),
                ))
                .height(Length::Fill),
        ]
        .spacing(2),
    )
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

fn agent_card(kind: AgentKind, state: &AgentState) -> Element<'_, Message> {
    let (dot, detail): (Color, String) = match state {
        AgentState::Detecting => (theme::TEXT_MUTED, "Detecting…".into()),
        AgentState::Missing => (
            theme::DISABLED,
            format!("`{}` not found on PATH", kind.program()),
        ),
        AgentState::Found(agent) => match &agent.version {
            Ok(v) => (theme::MINT, v.clone()),
            Err(e) => (theme::AMBER, format!("version unknown: {e}")),
        },
    };
    let found = matches!(state, AgentState::Found(_));
    let start = |label: &'static str, split: bool| {
        small_button(
            label,
            found.then_some(Message::Command(CommandId::StartAgent(kind, split))),
            false,
        )
    };
    let mut card = column![
        row![
            text("●").size(10).color(dot),
            text(kind.name()).size(13).color(theme::TEXT),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center),
        text(detail)
            .size(11)
            .color(theme::TEXT_MUTED)
            .wrapping(text::Wrapping::None),
    ]
    .spacing(3);
    if let AgentState::Found(agent) = state {
        card = card.push(
            text(agent.path.display().to_string())
                .size(10)
                .color(theme::DISABLED)
                .wrapping(text::Wrapping::None),
        );
    }
    card = card.push(row![start("Start in project", false), start("In split", true)].spacing(4));
    container(card)
        .padding(8)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(theme::SURFACE.into()),
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn context_row(item: &ContextItem) -> Element<'_, Message> {
    let label = row![
        container(text(item.kind.label()).size(10).color(theme::CYAN)).width(64),
        text(item.name.clone())
            .size(12)
            .color(theme::TEXT)
            .wrapping(text::Wrapping::None),
    ]
    .align_y(iced::Alignment::Center);
    let row_box = mouse_area(
        container(label)
            .padding([3, 4])
            .width(Length::Fill)
            .style(|_| container::Style {
                border: Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
    )
    .on_press(Message::AiInsert(item.path.clone()))
    .interaction(iced::mouse::Interaction::Pointer);
    let tip = match &item.description {
        Some(d) => format!("{d}\n{}", item.path.display()),
        None => item.path.display().to_string(),
    };
    tooltip(
        row_box,
        container(text(tip).size(11).color(theme::TEXT))
            .padding([3, 6])
            .max_width(320)
            .style(|_| container::Style {
                background: Some(theme::ELEVATED.into()),
                border: Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }),
        tooltip::Position::Right,
    )
    .into()
}

fn section(title: &str) -> Element<'_, Message> {
    text(title).size(10).color(theme::TEXT_MUTED).into()
}

fn muted<'a>(s: &'a str) -> Element<'a, Message> {
    text(s).size(11).color(theme::TEXT_MUTED).into()
}

fn small_button<'a>(label: &'a str, msg: Option<Message>, active: bool) -> Element<'a, Message> {
    let enabled = msg.is_some();
    button(text(label).size(12).color(if enabled {
        theme::TEXT
    } else {
        theme::DISABLED
    }))
    .padding([3, 8])
    .on_press_maybe(msg)
    .style(move |_, status| {
        let hovered =
            enabled && matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(
                if hovered || active {
                    theme::ELEVATED
                } else {
                    theme::GRAPHITE
                }
                .into(),
            ),
            text_color: if hovered { theme::CYAN } else { theme::TEXT },
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 5.0.into(),
            },
            ..Default::default()
        }
    })
    .into()
}
