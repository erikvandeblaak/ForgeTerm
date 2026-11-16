//! Mission Control: every AI agent in this window at a glance — what state it is in,
//! where it works, how long it runs, what it last printed, and what it costs the
//! machine. Everything shown is observed (see `forgeterm_agents::mission`); nothing
//! is estimated or invented. Actions are commands, like everywhere else.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use forgeterm_agents::AgentKind;
use forgeterm_agents::mission::{self, Status};
use forgeterm_git::Head;
use forgeterm_system::TreeUsage;
use forgeterm_terminal::Snapshot;
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Cache, Canvas, Frame, Geometry, Path as CanvasPath, Stroke};
use iced::widget::{button, column, container, mouse_area, row, scrollable, space, text, tooltip};
use iced::{
    Border, Color, Element, Length, Point, Rectangle, Renderer, Shadow, Size, Theme, Vector,
};

use crate::ai_view::AgentState;
use crate::app::Message;
use crate::commands::CommandId;
use crate::monitor_view::bytes;
use crate::terminal_pane::TermId;
use crate::terminal_view::{self, Metrics};
use crate::theme;

/// Width of one agent card; cards wrap to as many columns as fit.
pub const CARD_WIDTH: f32 = 470.0;
/// Samples of CPU history per agent (one per monitor sample, every 2 s while the
/// dashboard is open).
pub const CPU_HISTORY: usize = 60;
/// Finished sessions kept in the list.
pub const MAX_ENDED: usize = 12;
/// The activity dot fades out over this time after the last output.
const LIVE_FADE: Duration = Duration::from_secs(8);

#[derive(Default)]
pub struct Mission {
    pub visible: bool,
    /// Keyboard selection (arrow keys, Enter opens).
    pub selected: Option<TermId>,
    /// Checked-out branch per project root (`None` = not a repository).
    pub heads: HashMap<PathBuf, Option<Head>>,
    pub heads_busy: bool,
    /// Seconds the dashboard has been open (drives the branch refresh cadence).
    pub ticks: u32,
    /// CPU % of each agent's process tree, oldest first, with the pid it was
    /// measured for (a restarted agent starts a new history).
    pub cpu: HashMap<TermId, (u32, VecDeque<f32>)>,
    pub ended: VecDeque<Ended>,
}

impl Mission {
    pub fn record_cpu(&mut self, id: TermId, pid: u32, value: f32) {
        let (owner, h) = self.cpu.entry(id).or_insert_with(|| (pid, VecDeque::new()));
        if *owner != pid {
            *owner = pid;
            h.clear();
        }
        h.push_back(value);
        while h.len() > CPU_HISTORY {
            h.pop_front();
        }
    }

    pub fn record_ended(&mut self, ended: Ended) {
        self.ended.push_front(ended);
        self.ended.truncate(MAX_ENDED);
    }
}

/// A finished agent session.
#[derive(Debug, Clone)]
pub struct Ended {
    pub kind: AgentKind,
    pub project: String,
    pub ran: Option<Duration>,
    pub at: Instant,
    pub how: EndKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndKind {
    Exited(Option<i32>),
    FailedToStart,
    /// The user closed it while it was running.
    Closed,
}

/// Everything one card shows, gathered by the app from live state.
pub struct Card<'a> {
    pub id: TermId,
    pub kind: AgentKind,
    pub tab: usize,
    pub title: String,
    pub root: &'a Path,
    pub head: Option<&'a Head>,
    pub status: Status,
    /// What the agent itself said (OSC 7501 title/message), already sanitised.
    pub message: Option<String>,
    pub elapsed: Option<Duration>,
    pub since_output: Option<Duration>,
    pub usage: Option<&'a TreeUsage>,
    pub cpu: Option<&'a VecDeque<f32>>,
    pub snapshot: &'a Snapshot,
    pub cache: &'a Cache,
    pub selected: bool,
    pub focused: bool,
}

pub struct View<'a> {
    pub cards: Vec<Card<'a>>,
    pub mission: &'a Mission,
    pub agents: &'a [(AgentKind, AgentState)],
    /// Project root a new agent would start in.
    pub start_root: Option<&'a Path>,
    pub now: Instant,
    /// "Agent in a new worktree" is possible (trusted repository, agent found).
    pub worktree_ok: bool,
    /// The key that opens Mission Control (from the keymap; `None`: unbound).
    pub back_hint: Option<String>,
}

pub fn status_color(s: Status) -> Color {
    match s {
        Status::NeedsYou | Status::Bell => theme::amber(),
        Status::Failed => theme::coral(),
        Status::Working => theme::cyan(),
        Status::Done | Status::Running => theme::mint(),
        Status::Idle => theme::violet(),
        Status::Starting | Status::Exited => theme::text_muted(),
    }
}

pub fn view(v: View<'_>) -> Element<'_, Message> {
    let attention = v
        .cards
        .iter()
        .filter(|c| c.status.wants_attention())
        .count();
    let header = header(&v, attention);

    let mut body = column![].spacing(18).padding([4, 22]);
    if v.cards.is_empty() {
        body = body.push(empty_state(&v));
    } else {
        let mut cards = row![].spacing(14);
        for card in v.cards {
            cards = cards.push(card_view(card));
        }
        body = body.push(cards.wrap().vertical_spacing(14));
    }
    if !v.mission.ended.is_empty() {
        body = body.push(ended_list(&v.mission.ended, v.now));
    }
    body = body.push(
        text(
            "Status comes from the agent itself when it reports one; otherwise ForgeTerm \
             only shows facts: process state, output time and measured CPU/memory. \
             Esc closes · ←/→ select · Enter opens.",
        )
        .size(theme::size::TINY)
        .color(theme::disabled()),
    );

    container(
        column![
            header,
            scrollable(body)
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new().width(6).scroller_width(6),
                ))
                .height(Length::Fill),
        ]
        .spacing(theme::space::L),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|_| container::Style {
        background: Some(theme::obsidian().into()),
        ..Default::default()
    })
    .into()
}

fn header<'a>(v: &View<'a>, attention: usize) -> Element<'a, Message> {
    let running = v
        .cards
        .iter()
        .filter(|c| !matches!(c.status, Status::Exited | Status::Failed | Status::Starting))
        .count();
    let title = column![
        row![
            text("◆").size(16).color(theme::cyan()),
            text("Mission Control").size(20).color(theme::text()),
        ]
        .spacing(theme::space::L)
        .align_y(iced::Alignment::Center),
        text("Your AI agents in this window, live")
            .size(theme::size::SMALL)
            .color(theme::text_muted()),
    ]
    .spacing(theme::space::XXS);

    let mut chips = row![
        chip(format!("{running} running"), theme::mint(), running > 0),
        chip(
            format!(
                "{attention} need{} you",
                if attention == 1 { "s" } else { "" }
            ),
            theme::amber(),
            attention > 0
        ),
    ]
    .spacing(theme::space::M)
    .align_y(iced::Alignment::Center);
    if !v.mission.ended.is_empty() {
        chips = chips.push(chip(
            format!("{} ended", v.mission.ended.len()),
            theme::text_muted(),
            false,
        ));
    }

    let mut starts = row![]
        .spacing(theme::space::M)
        .align_y(iced::Alignment::Center);
    for (kind, state) in v.agents {
        starts = starts.push(start_button(*kind, state, v.start_root));
    }
    starts = starts.push(with_tip(
        action_button(
            "+ In new worktree…".to_owned(),
            v.worktree_ok
                .then_some(Message::Command(CommandId::NewAgentWorktree)),
            theme::cyan(),
            false,
        ),
        if v.worktree_ok {
            "New branch in its own folder, so agents work side by side".to_owned()
        } else {
            "Needs a Git repository (trusted) and an installed agent".to_owned()
        },
    ));

    container(
        row![
            title,
            space::horizontal(),
            chips,
            space::horizontal().width(24),
            starts
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding([16, 22])
    .width(Length::Fill)
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

/// Summary pill; `lit` = coloured, else muted.
pub fn chip<'a>(label: String, color: Color, lit: bool) -> Element<'a, Message> {
    let fg = if lit { color } else { theme::text_muted() };
    container(
        row![
            text("●").size(9).color(fg),
            text(label).size(theme::size::SMALL).color(fg)
        ]
        .spacing(theme::space::S)
        .align_y(iced::Alignment::Center),
    )
    .padding([4, 10])
    .style(move |_| container::Style {
        background: Some(
            Color {
                a: if lit { 0.12 } else { 0.0 },
                ..color
            }
            .into(),
        ),
        border: Border {
            color: if lit {
                Color { a: 0.45, ..color }
            } else {
                theme::border()
            },
            width: 1.0,
            radius: 12.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn start_button<'a>(
    kind: AgentKind,
    state: &'a AgentState,
    root: Option<&'a Path>,
) -> Element<'a, Message> {
    let found = matches!(state, AgentState::Found(_));
    let tip = match (state, root) {
        (AgentState::Found(_), Some(r)) => format!("Start {kind} in {}", r.display()),
        (AgentState::Found(_), None) => format!("Start {kind} in the project"),
        (AgentState::Detecting, _) => "Looking for the agent…".to_owned(),
        (AgentState::Missing, _) => format!("`{}` was not found on PATH", kind.program()),
    };
    let b = action_button(
        format!("+ {kind}"),
        found.then_some(Message::Command(CommandId::StartAgent(kind, false))),
        theme::cyan(),
        true,
    );
    with_tip(b, tip)
}

fn card_view(c: Card<'_>) -> Element<'_, Message> {
    let color = status_color(c.status);
    let live = mission::liveness(c.since_output, LIVE_FADE);
    let running = matches!(
        c.status,
        Status::Running
            | Status::Working
            | Status::Bell
            | Status::NeedsYou
            | Status::Done
            | Status::Idle
    );

    // Title row: agent, where it lives, status pill. Agents put their current task
    // in the terminal title (Claude Code does); shown as the agent sets it.
    let mut place = format!("tab {}", c.tab + 1);
    if c.focused {
        place.push_str(" · focused");
    }
    let task = c.title.trim();
    if !task.is_empty() && task != c.kind.name() {
        place.push_str(" · ");
        place.extend(task.chars().take(56));
        if task.chars().count() > 56 {
            place.push('…');
        }
    }
    let title_row = row![
        agent_badge(c.kind),
        column![
            text(c.kind.name())
                .size(theme::size::TITLE)
                .color(theme::text()),
            text(place)
                .size(theme::size::TINY)
                .color(theme::text_muted()),
        ]
        .spacing(1),
        space::horizontal(),
        with_tip(
            status_pill(c.status, if running { live } else { 0.0 }),
            c.status.source().to_owned()
        ),
    ]
    .spacing(theme::space::L)
    .align_y(iced::Alignment::Center);

    // Project, branch, folder.
    let project = c.root.file_name().map_or_else(
        || c.root.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let mut where_row = row![text(project).size(theme::size::BODY).color(theme::text())]
        .spacing(theme::space::M)
        .align_y(iced::Alignment::Center);
    match c.head {
        Some(h) => {
            let label = if h.detached {
                format!("⎇ detached @ {}", h.name)
            } else {
                format!("⎇ {}", h.name)
            };
            where_row = where_row.push(text(label).size(theme::size::SMALL).color(theme::violet()));
            if h.linked_worktree {
                where_row = where_row.push(tag("worktree", theme::violet()));
            }
        }
        None => {
            where_row = where_row.push(
                text("no git")
                    .size(theme::size::TINY)
                    .color(theme::disabled()),
            )
        }
    }
    // Long paths keep their end (the project), and never spill out of the card.
    let folder = container(
        text(tail(&c.root.display().to_string(), 68))
            .size(theme::size::TINY)
            .color(theme::text_muted())
            .wrapping(text::Wrapping::None),
    )
    .width(Length::Fill)
    .clip(true);

    // What the agent itself said, when it reports a status.
    let said: Option<Element<'_, Message>> = c.message.map(|m| {
        container(text(format!("“{m}”")).size(theme::size::SMALL).color(color))
            .padding([4, 8])
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Color { a: 0.08, ..color }.into()),
                border: Border {
                    radius: theme::radius::S.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
    });

    // Live miniature of the terminal: what the agent printed last.
    let preview = container(
        Canvas::new(Preview {
            snapshot: c.snapshot,
            cache: c.cache,
        })
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .height(150)
    .width(Length::Fill)
    .clip(true)
    .style(|_| container::Style {
        background: Some(theme::obsidian().into()),
        border: Border {
            color: theme::border(),
            width: 1.0,
            radius: theme::radius::M.into(),
        },
        ..Default::default()
    });
    let preview = mouse_area(preview)
        .on_press(Message::Command(CommandId::GoToTerminal(c.id)))
        .interaction(iced::mouse::Interaction::Pointer);
    let preview = with_tip(preview, "Open this terminal".to_owned());

    // Facts: time, output, resources.
    let elapsed = c
        .elapsed
        .map_or_else(|| "—".to_owned(), mission::format_duration);
    let recency = if running {
        mission::output_recency(c.since_output)
    } else {
        String::new()
    };
    let time = column![
        stat_label("RUNTIME"),
        text(elapsed).size(theme::size::LARGE).color(theme::text()),
        text(recency).size(theme::size::TINY).color(if live > 0.0 {
            theme::cyan()
        } else {
            theme::text_muted()
        }),
    ]
    .spacing(theme::space::XXS)
    .width(Length::FillPortion(3));

    let (cpu_now, mem, procs) = match c.usage {
        Some(u) => (
            u.cpu_percent
                .map_or_else(|| "measuring…".to_owned(), |p| format!("{p:.1} %")),
            bytes(u.memory),
            format!(
                "{} process{}",
                u.processes,
                if u.processes == 1 { "" } else { "es" }
            ),
        ),
        None if running => ("measuring…".to_owned(), "—".to_owned(), String::new()),
        None => ("—".to_owned(), "—".to_owned(), String::new()),
    };
    let cpu = column![
        stat_label("CPU"),
        row![
            text(cpu_now).size(theme::size::LARGE).color(theme::text()),
            Canvas::new(Spark { history: c.cpu })
                .width(Length::Fill)
                .height(20),
        ]
        .spacing(theme::space::M)
        .align_y(iced::Alignment::Center),
    ]
    .spacing(theme::space::XXS)
    .width(Length::FillPortion(4));
    let memory = column![
        stat_label("MEMORY"),
        text(mem).size(theme::size::LARGE).color(theme::text()),
        text(procs)
            .size(theme::size::TINY)
            .color(theme::text_muted()),
    ]
    .spacing(theme::space::XXS)
    .width(Length::FillPortion(3));
    let stats = with_tip(
        row![time, cpu, memory].spacing(14),
        "Measured by ForgeTerm: the agent's process and everything it started \
         (CPU as a share of the whole machine)."
            .to_owned(),
    );

    let stop_label = if running { "Stop" } else { "Close" };
    let actions = row![
        action_button(
            "Open".to_owned(),
            Some(Message::Command(CommandId::GoToTerminal(c.id))),
            theme::cyan(),
            true,
        ),
        with_tip(
            action_button(
                "Files".to_owned(),
                Some(Message::Command(CommandId::TerminalFolder(c.id))),
                theme::cyan(),
                false,
            ),
            "Show this agent's folder in the explorer".to_owned(),
        ),
        with_tip(
            action_button(
                "Changes".to_owned(),
                Some(Message::Command(CommandId::TerminalGit(c.id))),
                theme::cyan(),
                false,
            ),
            "Open the terminal with the Git panel: status, diffs, history".to_owned(),
        ),
        space::horizontal(),
        with_tip(
            action_button(
                stop_label.to_owned(),
                Some(Message::Command(CommandId::StopTerminal(c.id))),
                theme::coral(),
                false,
            ),
            if running {
                "Ends the agent (asks first)".to_owned()
            } else {
                "Closes this pane".to_owned()
            },
        ),
    ]
    .spacing(theme::space::S)
    .align_y(iced::Alignment::Center);

    let mut content = column![
        title_row,
        column![where_row, folder].spacing(theme::space::XXS)
    ]
    .spacing(theme::space::XL);
    if let Some(s) = said {
        content = content.push(s);
    }
    content = content.push(preview).push(stats).push(actions);

    let selected = c.selected;
    let attention = c.status.wants_attention();
    let card = container(
        row![
            // Status rail on the left edge of the card.
            container(space::vertical())
                .width(3)
                .height(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(color.into()),
                    border: Border {
                        radius: 2.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            container(content).padding([14, 16]).width(Length::Fill),
        ]
        .height(Length::Shrink),
    )
    .width(CARD_WIDTH)
    .style(move |_| container::Style {
        background: Some(theme::surface().into()),
        border: Border {
            color: if selected {
                theme::cyan()
            } else if attention {
                Color { a: 0.6, ..color }
            } else {
                theme::border()
            },
            width: if selected { 1.5 } else { 1.0 },
            radius: theme::radius::XL.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 14.0,
        },
        ..Default::default()
    });
    card.into()
}

pub fn agent_badge<'a>(kind: AgentKind) -> Element<'a, Message> {
    let (glyph, color) = match kind {
        AgentKind::ClaudeCode => ("✶", theme::amber()),
        AgentKind::Codex => ("◎", theme::cyan()),
    };
    container(text(glyph).size(16).color(color))
        .center_x(34)
        .center_y(34)
        .style(move |_| container::Style {
            background: Some(Color { a: 0.12, ..color }.into()),
            border: Border {
                color: Color { a: 0.35, ..color },
                width: 1.0,
                radius: theme::radius::L.into(),
            },
            ..Default::default()
        })
        .into()
}

fn status_pill<'a>(s: Status, live: f32) -> Element<'a, Message> {
    let color = status_color(s);
    // The dot glows right after output and fades (updated once per second).
    let dot = Color {
        a: 0.45 + 0.55 * live,
        ..color
    };
    container(
        row![
            text("●").size(theme::size::CAPTION).color(dot),
            text(s.label()).size(theme::size::SMALL).color(color),
        ]
        .spacing(theme::space::S)
        .align_y(iced::Alignment::Center),
    )
    .padding([4, 10])
    .style(move |_| container::Style {
        background: Some(Color { a: 0.12, ..color }.into()),
        border: Border {
            color: Color { a: 0.4, ..color },
            width: 1.0,
            radius: 12.0.into(),
        },
        ..Default::default()
    })
    .into()
}

pub fn tag<'a>(label: &'a str, color: Color) -> Element<'a, Message> {
    container(text(label).size(theme::size::CAPTION).color(color))
        .padding([1, 6])
        .style(move |_| container::Style {
            border: Border {
                color: Color { a: 0.5, ..color },
                width: 1.0,
                radius: theme::radius::S.into(),
            },
            ..Default::default()
        })
        .into()
}

pub fn stat_label<'a>(s: &'a str) -> Element<'a, Message> {
    text(s)
        .size(theme::size::CAPTION)
        .color(theme::text_muted())
        .into()
}

fn empty_state<'a>(v: &View<'a>) -> Element<'a, Message> {
    let mut agents = column![].spacing(theme::space::M);
    for (kind, state) in v.agents {
        let (dot, detail) = match state {
            AgentState::Detecting => (theme::text_muted(), "detecting…".to_owned()),
            AgentState::Missing => (theme::disabled(), "not installed (not on PATH)".to_owned()),
            AgentState::Found(a) => match &a.version {
                Ok(ver) => (theme::mint(), ver.clone()),
                Err(_) => (theme::amber(), "found, version unknown".to_owned()),
            },
        };
        agents = agents.push(
            row![
                agent_badge(*kind),
                column![
                    text(kind.name())
                        .size(theme::size::LARGE)
                        .color(theme::text()),
                    row![
                        text("●").size(9).color(dot),
                        text(detail)
                            .size(theme::size::TINY)
                            .color(theme::text_muted())
                    ]
                    .spacing(theme::space::S)
                    .align_y(iced::Alignment::Center),
                ]
                .spacing(theme::space::XXS),
                space::horizontal(),
                start_button(*kind, state, v.start_root),
            ]
            .spacing(theme::space::XL)
            .align_y(iced::Alignment::Center),
        );
    }
    let where_ = v.start_root.map_or_else(
        || "the folder of the focused terminal".to_owned(),
        |r| r.display().to_string(),
    );
    container(
        column![
            text("No agents running")
                .size(theme::size::HEADING)
                .color(theme::text()),
            text(format!(
                "Start one in {where_}. It opens in its own tab{}.",
                v.back_hint
                    .as_deref()
                    .map(|k| format!("; {k} brings you back here"))
                    .unwrap_or_default()
            ))
            .size(theme::size::SMALL)
            .color(theme::text_muted()),
            container(agents).padding([8, 0]),
        ]
        .spacing(theme::space::M),
    )
    .padding(22)
    .max_width(620)
    .style(|_| container::Style {
        background: Some(theme::surface().into()),
        border: Border {
            color: theme::border(),
            width: 1.0,
            radius: theme::radius::XL.into(),
        },
        ..Default::default()
    })
    .into()
}

fn ended_list(ended: &VecDeque<Ended>, now: Instant) -> Element<'_, Message> {
    let mut list = column![
        text("RECENTLY ENDED")
            .size(theme::size::CAPTION)
            .color(theme::text_muted())
    ]
    .spacing(theme::space::S);
    for e in ended {
        let (label, color) = match e.how {
            EndKind::Exited(Some(0)) => ("finished · exit 0".to_owned(), theme::mint()),
            EndKind::Exited(Some(c)) => (format!("exited with code {c}"), theme::coral()),
            EndKind::Exited(None) => ("ended (no exit code)".to_owned(), theme::amber()),
            EndKind::FailedToStart => ("failed to start".to_owned(), theme::coral()),
            EndKind::Closed => ("closed by you".to_owned(), theme::text_muted()),
        };
        let ran = e.ran.map_or_else(String::new, |d| {
            format!("ran {}", mission::format_duration(d))
        });
        let ago = format!(
            "{} ago",
            mission::format_duration(now.saturating_duration_since(e.at))
        );
        list = list.push(
            row![
                text("●").size(9).color(color),
                container(
                    text(e.kind.name())
                        .size(theme::size::SMALL)
                        .color(theme::text())
                )
                .width(96),
                container(
                    text(e.project.clone())
                        .size(theme::size::SMALL)
                        .color(theme::text())
                )
                .width(180),
                container(text(label).size(theme::size::SMALL).color(color)).width(170),
                container(
                    text(ran)
                        .size(theme::size::SMALL)
                        .color(theme::text_muted())
                )
                .width(110),
                text(ago)
                    .size(theme::size::SMALL)
                    .color(theme::text_muted()),
            ]
            .spacing(theme::space::L)
            .align_y(iced::Alignment::Center),
        );
    }
    container(list)
        .padding([12, 16])
        .max_width(CARD_WIDTH * 2.0 + 14.0)
        .style(|_| container::Style {
            background: Some(theme::graphite().into()),
            border: Border {
                color: theme::border(),
                width: 1.0,
                radius: theme::radius::XL.into(),
            },
            ..Default::default()
        })
        .into()
}

pub fn with_tip<'a>(content: impl Into<Element<'a, Message>>, tip: String) -> Element<'a, Message> {
    tooltip(
        content,
        container(text(tip).size(theme::size::TINY).color(theme::text()))
            .padding([4, 8])
            .max_width(340)
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
    )
    .gap(4)
    .into()
}

/// `primary`: filled accent style; otherwise quiet until hovered.
pub fn action_button<'a>(
    label: String,
    msg: Option<Message>,
    accent: Color,
    primary: bool,
) -> Element<'a, Message> {
    let enabled = msg.is_some();
    // Labels never wrap: a two-line button breaks the row's rhythm.
    button(
        text(label)
            .size(theme::size::SMALL)
            .wrapping(text::Wrapping::None),
    )
    .padding([5, 12])
    .on_press_maybe(msg)
    .style(move |_, status| {
        let hovered =
            enabled && matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (bg, fg, border) = if !enabled {
            (theme::graphite(), theme::disabled(), theme::border())
        } else if primary {
            (
                Color {
                    a: if hovered { 0.28 } else { 0.16 },
                    ..accent
                },
                accent,
                Color { a: 0.5, ..accent },
            )
        } else if hovered {
            (theme::elevated(), accent, Color { a: 0.5, ..accent })
        } else {
            (theme::graphite(), theme::text(), theme::border())
        };
        button::Style {
            background: Some(bg.into()),
            text_color: fg,
            border: Border {
                color: border,
                width: 1.0,
                radius: theme::radius::M.into(),
            },
            ..Default::default()
        }
    })
    .into()
}

/// The last `max` characters of `s`, with a leading ellipsis when cut.
pub fn tail(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_owned();
    }
    let keep: String = s.chars().skip(n - (max - 1)).collect();
    format!("…{keep}")
}

/// Rows a preview shows: the last `rows` up to the last line with text (or the
/// cursor), so a short session is not shown as empty space below the prompt.
pub fn preview_rows(snap: &Snapshot, rows: usize) -> std::ops::Range<usize> {
    let cols = snap.cols.max(1);
    let has_text = |r: usize| {
        snap.cells[r * cols..((r + 1) * cols).min(snap.cells.len())]
            .iter()
            .any(|c| c.c != ' ')
    };
    let last_text = (0..snap.rows).rev().find(|&r| has_text(r));
    let cursor = snap.cursor.as_ref().map(|c| c.row);
    let last = match (last_text, cursor) {
        (Some(a), Some(b)) => a.max(b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => 0,
    }
    .min(snap.rows.saturating_sub(1));
    let end = last + 1;
    end.saturating_sub(rows)..end
}

/// Read-only miniature of a terminal, scaled so its full width fits the card.
struct Preview<'a> {
    snapshot: &'a Snapshot,
    cache: &'a Cache,
}

impl canvas::Program<Message> for Preview<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let snap = self.snapshot;
        let geometry = self
            .cache
            .draw(renderer, bounds.size(), |frame: &mut Frame| {
                frame.fill_rectangle(
                    Point::ORIGIN,
                    bounds.size(),
                    theme::to_color(snap.background),
                );
                let pad = terminal_view::PADDING;
                // Fit the terminal's width; tiny text still reads as the familiar shape.
                let font = ((bounds.width - 2.0 * pad + 12.0) / (snap.cols.max(1) as f32 * 0.6))
                    .clamp(5.0, 9.0);
                let m = Metrics::new(font);
                let fit = (((bounds.height - 8.0) / m.cell_height).floor() as usize).max(1);
                let range = preview_rows(snap, fit);
                frame.with_save(|frame| {
                    // The renderer pads by PADDING; pull it in to 4 px on each side.
                    frame.translate(Vector::new(
                        4.0 - pad,
                        4.0 - pad - range.start as f32 * m.cell_height,
                    ));
                    let size = Size::new(
                        bounds.width + 2.0 * pad,
                        bounds.height + 2.0 * pad + range.start as f32 * m.cell_height,
                    );
                    terminal_view::draw_snapshot(frame, snap, m, size, false);
                });
            });
        vec![geometry]
    }
}

/// CPU history of one agent, auto-scaled (an agent mostly uses a few percent of
/// the machine; a 0–100 axis would be a flat line). The value is printed beside it.
struct Spark<'a> {
    history: Option<&'a VecDeque<f32>>,
}

impl canvas::Program<Message> for Spark<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        frame.fill_rectangle(Point::new(0.0, h - 1.0), Size::new(w, 1.0), theme::border());
        if let Some(history) = self.history.filter(|h| h.len() >= 2) {
            let max = history.iter().copied().fold(5.0_f32, f32::max);
            let n = history.len();
            let step = w / (CPU_HISTORY - 1) as f32;
            let point = |i: usize, v: f32| {
                Point::new(
                    w - (n - 1 - i) as f32 * step,
                    h - 1.0 - (v / max).clamp(0.0, 1.0) * (h - 3.0),
                )
            };
            let area = CanvasPath::new(|p| {
                p.move_to(Point::new(point(0, 0.0).x, h - 1.0));
                for (i, v) in history.iter().enumerate() {
                    p.line_to(point(i, *v));
                }
                p.line_to(Point::new(w, h - 1.0));
                p.close();
            });
            frame.fill(
                &area,
                Color {
                    a: 0.18,
                    ..theme::chart_cyan()
                },
            );
            let line = CanvasPath::new(|p| {
                for (i, v) in history.iter().enumerate() {
                    if i == 0 {
                        p.move_to(point(i, *v));
                    } else {
                        p.line_to(point(i, *v));
                    }
                }
            });
            frame.stroke(
                &line,
                Stroke::default()
                    .with_color(theme::chart_cyan())
                    .with_width(1.5),
            );
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forgeterm_terminal::{CellView, Rgb};

    fn snap(lines: &[&str], cols: usize, rows: usize) -> Snapshot {
        let blank = CellView {
            c: ' ',
            zerowidth: None,
            fg: Rgb { r: 0, g: 0, b: 0 },
            bg: Rgb { r: 0, g: 0, b: 0 },
            flags: Default::default(),
        };
        let mut cells = vec![blank; cols * rows];
        for (r, line) in lines.iter().enumerate() {
            for (c, ch) in line.chars().enumerate() {
                cells[r * cols + c].c = ch;
            }
        }
        Snapshot {
            cols,
            rows,
            cells,
            cursor: None,
            background: Rgb { r: 0, g: 0, b: 0 },
            display_offset: 0,
            history: 0,
            prompt_marks: Vec::new(),
        }
    }

    #[test]
    fn preview_ends_at_the_last_text() {
        let s = snap(&["a", "b", "c", ""], 4, 10);
        assert_eq!(preview_rows(&s, 2), 1..3);
        assert_eq!(preview_rows(&s, 8), 0..3);
        let full = snap(&["1", "2", "3", "4", "5"], 2, 5);
        assert_eq!(preview_rows(&full, 3), 2..5);
        let empty = snap(&[], 3, 4);
        assert_eq!(preview_rows(&empty, 3), 0..1);
    }

    #[test]
    fn long_paths_keep_their_end() {
        assert_eq!(tail(r"C:\work\shop", 20), r"C:\work\shop");
        let t = tail("abcdefghij", 5);
        assert_eq!(t, "…ghij");
        assert_eq!(t.chars().count(), 5);
        assert_eq!(tail("ééééé", 3), "…éé");
    }

    #[test]
    fn histories_are_bounded() {
        let mut m = Mission::default();
        for i in 0..(CPU_HISTORY + 10) {
            m.record_cpu(1, 42, i as f32);
        }
        assert_eq!(m.cpu[&1].1.len(), CPU_HISTORY);
        assert_eq!(m.cpu[&1].1.back(), Some(&((CPU_HISTORY + 9) as f32)));
        // A restarted agent (new pid) starts a fresh history.
        m.record_cpu(1, 43, 7.0);
        assert_eq!(m.cpu[&1].1.iter().copied().collect::<Vec<_>>(), vec![7.0]);
        for _ in 0..(MAX_ENDED + 3) {
            m.record_ended(Ended {
                kind: AgentKind::Codex,
                project: "p".into(),
                ran: None,
                at: Instant::now(),
                how: EndKind::Closed,
            });
        }
        assert_eq!(m.ended.len(), MAX_ENDED);
    }
}
