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
                theme::obsidian()
            } else {
                theme::graphite()
            }
            .into(),
        ),
        border: Border {
            color: if active {
                theme::cyan()
            } else {
                theme::border()
            },
            width: 1.0,
            radius: theme::radius::M.into(),
        },
        ..Default::default()
    }
}

pub(super) fn ghost_button(status: button::Status, hover: Color) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then(|| theme::elevated().into()),
        text_color: if hovered { hover } else { theme::text() },
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
                    _ => theme::elevated(),
                }
                .into(),
            ),
            text_color: match status {
                button::Status::Hovered | button::Status::Pressed => theme::obsidian(),
                _ => theme::text(),
            },
            border: Border {
                color: accent,
                width: 1.0,
                radius: theme::radius::M.into(),
            },
            ..Default::default()
        }
    };
    container(
        row![
            text("⚠").size(16).color(theme::amber()),
            column![
                text(format!("Paste {lines} lines?")).size(theme::size::BODY).color(theme::text()),
                text("This shell has not enabled bracketed paste: every line will run as soon as it is pasted.")
                    .size(theme::size::SMALL)
                    .color(theme::text_muted()),
            ]
            .spacing(theme::space::XXS),
            space::horizontal(),
            button(text("Paste and run (Enter)").size(theme::size::SMALL))
                .on_press(Message::ConfirmPaste)
                .padding([6, 12])
                .style(button_style(theme::amber())),
            button(text("Cancel (Esc)").size(theme::size::SMALL))
                .on_press(Message::CancelPaste)
                .padding([6, 12])
                .style(button_style(theme::cyan())),
        ]
        .spacing(theme::space::XL)
        .align_y(iced::Alignment::Center),
    )
    .padding([10, 14])
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(theme::surface().into()),
        border: Border { color: theme::amber(), width: 1.0, radius: 0.0.into() },
        ..Default::default()
    })
    .into()
}

/// Shown while files from the OS are dragged over the window.
pub(super) fn drop_hint(shell: &str) -> Element<'_, Message> {
    let card = container(
        column![
            text("Drop to insert the path")
                .size(16)
                .color(theme::text()),
            text(format!(
                "Quoted for {shell} · inserted at the prompt, not executed"
            ))
            .size(theme::size::SMALL)
            .color(theme::text_muted()),
        ]
        .spacing(theme::space::XS)
        .align_x(iced::Alignment::Center),
    )
    .padding([16, 24])
    .style(|_| container::Style {
        background: Some(theme::surface().into()),
        border: Border {
            color: theme::cyan(),
            width: 2.0,
            radius: theme::radius::XL.into(),
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
            text("!").size(theme::size::TITLE).color(theme::amber()),
            text(message).size(theme::size::SMALL).color(theme::text()),
            space::horizontal(),
            button(text("Dismiss").size(theme::size::SMALL))
                .on_press(Message::DismissNotice)
                .padding([4, 10])
                .style(|_, status| ghost_button(status, theme::cyan())),
        ]
        .spacing(theme::space::L)
        .align_y(iced::Alignment::Center),
    )
    .padding([6, 14])
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(theme::surface().into()),
        border: Border {
            color: theme::amber(),
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
        button(text(label).size(theme::size::SMALL))
            .on_press(msg)
            .padding([3, 9])
            .style(|_, status| ghost_button(status, theme::cyan()))
    };
    container(
        row![
            text("Find")
                .size(theme::size::SMALL)
                .color(theme::text_muted()),
            text_input("in terminal output", &f.query)
                .id(iced::widget::Id::new(TERM_FIND_ID))
                .on_input(Message::TermFindQuery)
                .on_submit(Message::TermFindStep(true))
                .size(theme::size::BODY)
                .padding([4, 8])
                .width(Length::Fixed(320.0))
                .style(move |_, status| text_input::Style {
                    background: theme::obsidian().into(),
                    border: Border {
                        color: if not_found {
                            theme::coral()
                        } else if matches!(status, text_input::Status::Focused { .. }) {
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
                }),
            small("↑", Message::TermFindStep(true)),
            small("↓", Message::TermFindStep(false)),
            text(status).size(theme::size::SMALL).color(if not_found {
                theme::coral()
            } else {
                theme::text_muted()
            }),
            space::horizontal(),
            small("× (Esc)", Message::TermFindClose),
        ]
        .spacing(theme::space::M)
        .align_y(iced::Alignment::Center),
    )
    .padding([6, 12])
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(theme::surface().into()),
        border: Border {
            color: theme::border(),
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
    let mut list = column![].spacing(theme::space::XS);
    for p in programs.iter().take(8) {
        list = list.push(
            text(format!("• {p}"))
                .size(theme::size::BODY)
                .color(theme::text()),
        );
    }
    if programs.len() > 8 {
        list = list.push(
            text(format!("… and {} more", programs.len() - 8))
                .size(theme::size::SMALL)
                .color(theme::text_muted()),
        );
    }
    let button_style = |accent: Color| {
        move |_: &iced::Theme, status: button::Status| button::Style {
            background: Some(
                match status {
                    button::Status::Hovered | button::Status::Pressed => accent,
                    _ => theme::elevated(),
                }
                .into(),
            ),
            text_color: match status {
                button::Status::Hovered | button::Status::Pressed => theme::obsidian(),
                _ => theme::text(),
            },
            border: Border {
                color: accent,
                width: 1.0,
                radius: theme::radius::M.into(),
            },
            ..Default::default()
        }
    };
    let card = container(
        column![
            text(title).size(theme::size::HEADING).color(theme::text()),
            text(if programs.len() == 1 {
                "This program is still running and will be ended:"
            } else {
                "These programs are still running and will be ended:"
            })
            .size(theme::size::BODY)
            .color(theme::text_muted()),
            list,
            row![
                space::horizontal(),
                button(text("Cancel (Esc)").size(theme::size::BODY))
                    .on_press(Message::CancelClose)
                    .padding([6, 14])
                    .style(button_style(theme::cyan())),
                button(text(action).size(theme::size::BODY))
                    .on_press(Message::ConfirmClose)
                    .padding([6, 14])
                    .style(button_style(theme::coral())),
            ]
            .spacing(theme::space::M),
        ]
        .spacing(theme::space::XL),
    )
    .padding(22)
    .width(Length::Fixed(480.0))
    .style(|_| container::Style {
        background: Some(theme::surface().into()),
        border: Border {
            color: theme::amber(),
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
        .style(|_| container::Style {
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.45).into()),
            ..Default::default()
        });
    opaque(mouse_area(backdrop).on_press(Message::CancelClose))
}

/// Help → About: version, creator and the open-source components ForgeTerm is built on.
pub(super) fn view_about<'a>() -> Element<'a, Message> {
    let muted = |s: &'static str| text(s).size(theme::size::SMALL).color(theme::text_muted());
    let close = button(text("Close (Esc)").size(theme::size::SMALL))
        .on_press(Message::AboutClose)
        .padding([6, 14])
        .style(|_, status| ghost_button(status, theme::cyan()));
    let card = container(
        column![
            row![
                svg(svg::Handle::from_memory(LOGO)).width(44).height(44),
                column![
                    text("ForgeTerm").size(22).color(theme::text()),
                    text(format!("Version {}", env!("CARGO_PKG_VERSION")))
                        .size(theme::size::SMALL)
                        .color(theme::text_muted()),
                ]
                .spacing(theme::space::XXS),
            ]
            .spacing(14)
            .align_y(iced::Alignment::Center),
            text("Erik van de Blaak Edition")
                .size(theme::size::BODY)
                .color(theme::cyan()),
            text("Created by Erik van de Blaak")
                .size(theme::size::BODY)
                .color(theme::text()),
            button(text(crate::commands::WEBSITE).size(theme::size::BODY))
                .on_press(Message::Command(CommandId::Website))
                .padding(0)
                .style(|_, status| link_button(status)),
            column![
                muted("Built with Rust, iced 0.14 (MIT) and alacritty_terminal 0.26 (Apache-2.0)."),
                muted("Terminal font: JetBrains Mono (SIL Open Font License 1.1)."),
            ]
            .spacing(theme::space::XS),
            row![space::horizontal(), close],
        ]
        .spacing(14),
    )
    .padding(22)
    .width(Length::Fixed(460.0))
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
        .size(theme::size::BODY)
        .color(theme::text())
    ]
    .spacing(theme::space::XXS);
    for p in problems.iter().take(SHOWN) {
        lines = lines.push(
            text(format!("• {p}"))
                .size(theme::size::SMALL)
                .color(theme::text_muted()),
        );
    }
    if problems.len() > SHOWN {
        lines = lines.push(
            text(format!(
                "… and {} more (see the log)",
                problems.len() - SHOWN
            ))
            .size(theme::size::SMALL)
            .color(theme::text_muted()),
        );
    }
    let action = |label: &'static str, msg: Message| {
        button(text(label).size(theme::size::SMALL))
            .on_press(msg)
            .padding([5, 10])
            .style(|_, status| ghost_button(status, theme::cyan()))
    };
    container(
        row![
            text("⚙").size(16).color(theme::amber()),
            lines,
            space::horizontal(),
            action("Open settings", Message::Command(CommandId::OpenSettings)),
            action("Dismiss", Message::DismissSettingsProblems),
        ]
        .spacing(theme::space::XL)
        .align_y(iced::Alignment::Center),
    )
    .padding([8, 14])
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(theme::surface().into()),
        border: Border {
            color: theme::amber(),
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
            button::Status::Hovered | button::Status::Pressed => theme::text(),
            _ => theme::cyan(),
        },
        ..Default::default()
    }
}

pub(super) fn bar_style() -> container::Style {
    container::Style {
        background: Some(theme::graphite().into()),
        border: Border {
            color: theme::border(),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}
