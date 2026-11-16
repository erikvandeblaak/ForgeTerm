//! Small view pieces without App state.

use super::*;
use iced::widget::column;

/// Status-bar form of a directory: home as `~`, long paths cut at the front.
pub(super) fn short_path(path: &Path) -> String {
    const MAX: usize = 48;
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    let shown = match home.as_deref().and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    };
    let count = shown.chars().count();
    if count <= MAX {
        shown
    } else {
        let tail: String = shown.chars().skip(count - (MAX - 1)).collect();
        format!("…{tail}")
    }
}

pub(super) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

pub(super) fn tab_style(active: bool) -> container::Style {
    container::Style {
        background: Some(
            if active {
                theme::OBSIDIAN
            } else {
                theme::GRAPHITE
            }
            .into(),
        ),
        border: Border {
            color: if active { theme::CYAN } else { theme::BORDER },
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

pub(super) fn ghost_button(status: button::Status, hover: Color) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then(|| theme::ELEVATED.into()),
        text_color: if hovered { hover } else { theme::TEXT },
        border: Border {
            radius: 5.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub(super) fn paste_banner(text_to_paste: &str) -> Element<'_, Message> {
    let lines = text_to_paste.trim_end_matches(['\r', '\n']).lines().count();
    let button_style = |accent: Color| {
        move |_: &iced::Theme, status: button::Status| button::Style {
            background: Some(
                match status {
                    button::Status::Hovered | button::Status::Pressed => accent,
                    _ => theme::ELEVATED,
                }
                .into(),
            ),
            text_color: match status {
                button::Status::Hovered | button::Status::Pressed => theme::OBSIDIAN,
                _ => theme::TEXT,
            },
            border: Border {
                color: accent,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        }
    };
    container(
        row![
            text("⚠").size(16).color(theme::AMBER),
            column![
                text(format!("Paste {lines} lines?")).size(13).color(theme::TEXT),
                text("This shell has not enabled bracketed paste: every line will run as soon as it is pasted.")
                    .size(12)
                    .color(theme::TEXT_MUTED),
            ]
            .spacing(2),
            space::horizontal(),
            button(text("Paste and run (Enter)").size(12))
                .on_press(Message::ConfirmPaste)
                .padding([6, 12])
                .style(button_style(theme::AMBER)),
            button(text("Cancel (Esc)").size(12))
                .on_press(Message::CancelPaste)
                .padding([6, 12])
                .style(button_style(theme::CYAN)),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center),
    )
    .padding([10, 14])
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(theme::SURFACE.into()),
        border: Border { color: theme::AMBER, width: 1.0, radius: 0.0.into() },
        ..Default::default()
    })
    .into()
}

/// Shown while files from the OS are dragged over the window.
pub(super) fn drop_hint(shell: &str) -> Element<'_, Message> {
    let card = container(
        column![
            text("Drop to insert the path").size(16).color(theme::TEXT),
            text(format!(
                "Quoted for {shell} · inserted at the prompt, not executed"
            ))
            .size(12)
            .color(theme::TEXT_MUTED),
        ]
        .spacing(4)
        .align_x(iced::Alignment::Center),
    )
    .padding([16, 24])
    .style(|_| container::Style {
        background: Some(theme::SURFACE.into()),
        border: Border {
            color: theme::CYAN,
            width: 2.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    });
    container(card)
        .center(Length::Fill)
        .style(|_| container::Style {
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.3).into()),
            ..Default::default()
        })
        .into()
}

/// A dismissible one-line problem report.
pub(super) fn notice_banner(message: &str) -> Element<'_, Message> {
    container(
        row![
            text("!").size(15).color(theme::AMBER),
            text(message).size(12).color(theme::TEXT),
            space::horizontal(),
            button(text("Dismiss").size(12))
                .on_press(Message::DismissNotice)
                .padding([4, 10])
                .style(|_, status| ghost_button(status, theme::CYAN)),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center),
    )
    .padding([6, 14])
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(theme::SURFACE.into()),
        border: Border {
            color: theme::AMBER,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

/// Find bar above the terminals (Ctrl+Shift+F).
pub(super) fn term_find_bar(f: &TermFind) -> Element<'_, Message> {
    let status = match (f.query.is_empty(), f.outcome) {
        (true, _) => "Type to search the output and scrollback",
        (false, Some(forgeterm_terminal::SearchOutcome::NotFound)) => "Not found",
        (false, Some(forgeterm_terminal::SearchOutcome::Found)) => {
            "Found · Enter/F3 older · Shift+F3 newer · Ctrl+C copies"
        }
        (false, None) => "",
    };
    let not_found = f.outcome == Some(forgeterm_terminal::SearchOutcome::NotFound);
    let small = |label: &'static str, msg: Message| {
        button(text(label).size(12))
            .on_press(msg)
            .padding([3, 9])
            .style(|_, status| ghost_button(status, theme::CYAN))
    };
    container(
        row![
            text("Find").size(12).color(theme::TEXT_MUTED),
            text_input("in terminal output", &f.query)
                .id(iced::widget::Id::new(TERM_FIND_ID))
                .on_input(Message::TermFindQuery)
                .on_submit(Message::TermFindStep(true))
                .size(13)
                .padding([4, 8])
                .width(Length::Fixed(320.0))
                .style(move |_, status| text_input::Style {
                    background: theme::OBSIDIAN.into(),
                    border: Border {
                        color: if not_found {
                            theme::CORAL
                        } else if matches!(status, text_input::Status::Focused { .. }) {
                            theme::CYAN
                        } else {
                            theme::BORDER
                        },
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    icon: theme::TEXT_MUTED,
                    placeholder: theme::TEXT_MUTED,
                    value: theme::TEXT,
                    selection: theme::SELECTION,
                }),
            small("↑", Message::TermFindStep(true)),
            small("↓", Message::TermFindStep(false)),
            text(status).size(12).color(if not_found {
                theme::CORAL
            } else {
                theme::TEXT_MUTED
            }),
            space::horizontal(),
            small("× (Esc)", Message::TermFindClose),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    )
    .padding([6, 12])
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(theme::SURFACE.into()),
        border: Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

/// "Programs are still running" confirmation for quit / close tab / close pane.
pub(super) fn view_close_confirm<'a>(
    target: &CloseTarget,
    programs: &'a [String],
) -> Element<'a, Message> {
    let (title, action) = match target {
        CloseTarget::Quit => ("Quit ForgeTerm?", "Quit (Enter)"),
        CloseTarget::Tab(_) => ("Close this tab?", "Close tab (Enter)"),
        CloseTarget::Pane(_) => ("Close this pane?", "Close pane (Enter)"),
    };
    let mut list = column![].spacing(4);
    for p in programs.iter().take(8) {
        list = list.push(text(format!("• {p}")).size(13).color(theme::TEXT));
    }
    if programs.len() > 8 {
        list = list.push(
            text(format!("… and {} more", programs.len() - 8))
                .size(12)
                .color(theme::TEXT_MUTED),
        );
    }
    let button_style = |accent: Color| {
        move |_: &iced::Theme, status: button::Status| button::Style {
            background: Some(
                match status {
                    button::Status::Hovered | button::Status::Pressed => accent,
                    _ => theme::ELEVATED,
                }
                .into(),
            ),
            text_color: match status {
                button::Status::Hovered | button::Status::Pressed => theme::OBSIDIAN,
                _ => theme::TEXT,
            },
            border: Border {
                color: accent,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        }
    };
    let card = container(
        column![
            text(title).size(18).color(theme::TEXT),
            text(if programs.len() == 1 {
                "This program is still running and will be ended:"
            } else {
                "These programs are still running and will be ended:"
            })
            .size(13)
            .color(theme::TEXT_MUTED),
            list,
            row![
                space::horizontal(),
                button(text("Cancel (Esc)").size(13))
                    .on_press(Message::CancelClose)
                    .padding([6, 14])
                    .style(button_style(theme::CYAN)),
                button(text(action).size(13))
                    .on_press(Message::ConfirmClose)
                    .padding([6, 14])
                    .style(button_style(theme::CORAL)),
            ]
            .spacing(8),
        ]
        .spacing(12),
    )
    .padding(22)
    .width(Length::Fixed(480.0))
    .style(|_| container::Style {
        background: Some(theme::SURFACE.into()),
        border: Border {
            color: theme::AMBER,
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
        .style(|_| container::Style {
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.45).into()),
            ..Default::default()
        });
    opaque(mouse_area(backdrop).on_press(Message::CancelClose))
}

/// Help → About: version, creator and the open-source components ForgeTerm is built on.
pub(super) fn view_about<'a>() -> Element<'a, Message> {
    let muted = |s: &'static str| text(s).size(12).color(theme::TEXT_MUTED);
    let close = button(text("Close (Esc)").size(12))
        .on_press(Message::AboutClose)
        .padding([6, 14])
        .style(|_, status| ghost_button(status, theme::CYAN));
    let card = container(
        column![
            row![
                svg(svg::Handle::from_memory(LOGO)).width(44).height(44),
                column![
                    text("ForgeTerm").size(22).color(theme::TEXT),
                    text(format!("Version {}", env!("CARGO_PKG_VERSION")))
                        .size(12)
                        .color(theme::TEXT_MUTED),
                ]
                .spacing(2),
            ]
            .spacing(14)
            .align_y(iced::Alignment::Center),
            text("Erik van de Blaak Edition")
                .size(13)
                .color(theme::CYAN),
            text("Created by Erik van de Blaak")
                .size(13)
                .color(theme::TEXT),
            button(text(crate::commands::WEBSITE).size(13))
                .on_press(Message::Command(CommandId::Website))
                .padding(0)
                .style(|_, status| link_button(status)),
            column![
                muted("Built with Rust, iced 0.14 (MIT) and alacritty_terminal 0.26 (Apache-2.0)."),
                muted("Terminal font: JetBrains Mono (SIL Open Font License 1.1)."),
            ]
            .spacing(4),
            row![space::horizontal(), close],
        ]
        .spacing(14),
    )
    .padding(22)
    .width(Length::Fixed(460.0))
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
        .style(|_| container::Style {
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.35).into()),
            ..Default::default()
        });
    opaque(mouse_area(backdrop).on_press(Message::AboutClose))
}

/// Problems found in settings.toml (each setting fell back to its default).
pub(super) fn settings_banner(problems: &[String]) -> Element<'_, Message> {
    const SHOWN: usize = 3;
    let mut lines = column![
        text(format!(
            "settings.toml: {} problem{}",
            problems.len(),
            if problems.len() == 1 { "" } else { "s" }
        ))
        .size(13)
        .color(theme::TEXT)
    ]
    .spacing(2);
    for p in problems.iter().take(SHOWN) {
        lines = lines.push(text(format!("• {p}")).size(12).color(theme::TEXT_MUTED));
    }
    if problems.len() > SHOWN {
        lines = lines.push(
            text(format!(
                "… and {} more (see the log)",
                problems.len() - SHOWN
            ))
            .size(12)
            .color(theme::TEXT_MUTED),
        );
    }
    let action = |label: &'static str, msg: Message| {
        button(text(label).size(12))
            .on_press(msg)
            .padding([5, 10])
            .style(|_, status| ghost_button(status, theme::CYAN))
    };
    container(
        row![
            text("⚙").size(16).color(theme::AMBER),
            lines,
            space::horizontal(),
            action("Open settings", Message::Command(CommandId::OpenSettings)),
            action("Dismiss", Message::DismissSettingsProblems),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center),
    )
    .padding([8, 14])
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(theme::SURFACE.into()),
        border: Border {
            color: theme::AMBER,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

/// A text link: cyan, brighter on hover, no background.
pub(super) fn link_button(status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: match status {
            button::Status::Hovered | button::Status::Pressed => theme::TEXT,
            _ => theme::CYAN,
        },
        ..Default::default()
    }
}

pub(super) fn bar_style() -> container::Style {
    container::Style {
        background: Some(theme::GRAPHITE.into()),
        border: Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}
