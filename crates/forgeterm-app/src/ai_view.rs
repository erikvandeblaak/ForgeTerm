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
        text("AI WORKSPACE")
            .size(theme::size::TINY)
            .color(theme::text_muted()),
        space::horizontal(),
        small_button("↻", Some(Message::Command(CommandId::AiRefresh)), false),
    ]
    .align_y(iced::Alignment::Center);

    let mut body = column![].spacing(theme::space::L);
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
                    text(name).size(theme::size::BODY).color(theme::text()),
                    text(ctx.root.display().to_string())
                        .size(theme::size::TINY)
                        .color(theme::text_muted())
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
        .spacing(theme::space::XXS),
    )
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

fn agent_card(kind: AgentKind, state: &AgentState) -> Element<'_, Message> {
    let (dot, detail): (Color, String) = match state {
        AgentState::Detecting => (theme::text_muted(), "Detecting…".into()),
        AgentState::Missing => (
            theme::disabled(),
            format!("`{}` not found on PATH", kind.program()),
        ),
        AgentState::Found(agent) => match &agent.version {
            Ok(v) => (theme::mint(), v.clone()),
            Err(e) => (theme::amber(), format!("version unknown: {e}")),
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
            text("●").size(theme::size::CAPTION).color(dot),
            text(kind.name())
                .size(theme::size::BODY)
                .color(theme::text()),
        ]
        .spacing(theme::space::S)
        .align_y(iced::Alignment::Center),
        text(detail)
            .size(theme::size::TINY)
            .color(theme::text_muted())
            .wrapping(text::Wrapping::None),
    ]
    .spacing(3);
    if let AgentState::Found(agent) = state {
        card = card.push(
            text(agent.path.display().to_string())
                .size(theme::size::CAPTION)
                .color(theme::disabled())
                .wrapping(text::Wrapping::None),
        );
    }
    card = card.push(
        row![start("Start in project", false), start("In split", true)].spacing(theme::space::XS),
    );
    container(card)
        .padding(8)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(theme::surface().into()),
            border: Border {
                color: theme::border(),
                width: 1.0,
                radius: theme::radius::M.into(),
            },
            ..Default::default()
        })
        .into()
}

fn context_row(item: &ContextItem) -> Element<'_, Message> {
    let label = row![
        container(
            text(item.kind.label())
                .size(theme::size::CAPTION)
                .color(theme::cyan())
        )
        .width(64),
        text(item.name.clone())
            .size(theme::size::SMALL)
            .color(theme::text())
            .wrapping(text::Wrapping::None),
    ]
    .align_y(iced::Alignment::Center);
    let row_box = mouse_area(
        container(label)
            .padding([3, 4])
            .width(Length::Fill)
            .style(|_| container::Style {
                border: Border {
                    radius: theme::radius::S.into(),
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
        container(text(tip).size(theme::size::TINY).color(theme::text()))
            .padding([3, 6])
            .max_width(320)
            .style(|_| container::Style {
                background: Some(theme::elevated().into()),
                border: Border {
                    color: theme::border(),
                    width: 1.0,
                    radius: theme::radius::S.into(),
                },
                ..Default::default()
            }),
        tooltip::Position::Right,
    )
    .into()
}

fn section(title: &str) -> Element<'_, Message> {
    text(title)
        .size(theme::size::CAPTION)
        .color(theme::text_muted())
        .into()
}

fn muted<'a>(s: &'a str) -> Element<'a, Message> {
    text(s)
        .size(theme::size::TINY)
        .color(theme::text_muted())
        .into()
}

fn small_button<'a>(label: &'a str, msg: Option<Message>, active: bool) -> Element<'a, Message> {
    let enabled = msg.is_some();
    button(text(label).size(theme::size::SMALL).color(if enabled {
        theme::text()
    } else {
        theme::disabled()
    }))
    .padding([3, 8])
    .on_press_maybe(msg)
    .style(move |_, status| {
        let hovered =
            enabled && matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(
                if hovered || active {
                    theme::elevated()
                } else {
                    theme::graphite()
                }
                .into(),
            ),
            text_color: if hovered {
                theme::cyan()
            } else {
                theme::text()
            },
            border: Border {
                color: theme::border(),
                width: 1.0,
                radius: 5.0.into(),
            },
            ..Default::default()
        }
    })
    .into()
}
