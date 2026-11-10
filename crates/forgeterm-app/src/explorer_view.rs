//! Explorer side panel (view only; state lives in `forgeterm_explorer::Tree`).

use std::path::Path;

use forgeterm_explorer::{Row, Tree};
use iced::widget::{button, column, container, mouse_area, row, scrollable, space, text, tooltip};
use iced::{Border, Element, Length};

use crate::app::Message;
use crate::commands::CommandId;
use crate::theme;

pub const WIDTH: f32 = 270.0;
const INDENT: f32 = 14.0;

pub fn view(tree: &Tree, follow: bool) -> Element<'_, Message> {
    let root_name = folder_name(tree.root());
    let header = row![
        text("EXPLORER").size(11).color(theme::TEXT_MUTED),
        space::horizontal(),
        icon_button(
            "↑",
            "Go to Parent Folder",
            CommandId::ExplorerParent,
            tree.parent().is_some(),
            false
        ),
        icon_button(
            "cd",
            "Follow Terminal Folder",
            CommandId::ExplorerToggleFollow,
            true,
            follow
        ),
        icon_button("↻", "Refresh", CommandId::ExplorerRefresh, true, false),
        icon_button(
            ".*",
            "Show Hidden Files",
            CommandId::ExplorerToggleHidden,
            true,
            tree.show_hidden()
        ),
    ]
    .spacing(2)
    .align_y(iced::Alignment::Center);

    let root = column![
        text(root_name).size(13).color(theme::TEXT),
        text(tree.root().to_string_lossy().into_owned())
            .size(11)
            .color(theme::TEXT_MUTED)
            .wrapping(text::Wrapping::None),
    ]
    .spacing(1);

    let selected = tree.selected().map(|e| e.path.as_path());
    let mut list = column![].spacing(0);
    for r in tree.rows() {
        list = list.push(view_row(&r, selected));
    }
    let list = scrollable(container(list).padding([0, 4]))
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new().width(6).scroller_width(6),
        ))
        .height(Length::Fill)
        .width(Length::Fill);

    let mut panel = column![
        container(column![header, root].spacing(6)).padding([8, 10]),
        list
    ]
    .spacing(2);
    if let Some(entry) = tree.selected() {
        panel = panel.push(footer(&entry.name, entry.is_dir));
    }

    container(panel)
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

fn view_row<'a>(r: &Row<'a>, selected: Option<&Path>) -> Element<'a, Message> {
    match r {
        Row::Note { depth, text: note } => row![
            space::horizontal().width(indent(*depth) + 16.0),
            text(note.clone()).size(12).color(theme::TEXT_MUTED),
        ]
        .height(22)
        .align_y(iced::Alignment::Center)
        .into(),
        Row::Entry {
            depth,
            entry,
            expanded,
        } => {
            let is_selected = selected == Some(entry.path.as_path());
            let color = if entry.hidden {
                theme::TEXT_MUTED
            } else {
                theme::TEXT
            };
            let chevron: Element<'a, Message> = if entry.is_dir {
                mouse_area(
                    container(
                        text(if *expanded { "▾" } else { "▸" })
                            .size(15)
                            .color(theme::TEXT_MUTED),
                    )
                    .center_x(16),
                )
                .on_press(Message::ExplorerToggle(entry.path.clone()))
                .into()
            } else {
                space::horizontal().width(16).into()
            };
            let label = row![
                space::horizontal().width(indent(*depth)),
                chevron,
                text(entry.name.clone())
                    .size(13)
                    .color(color)
                    .wrapping(text::Wrapping::None),
            ]
            .align_y(iced::Alignment::Center);
            let body = container(label)
                .height(22)
                .width(Length::Fill)
                .align_y(iced::Alignment::Center)
                .style(move |_| container::Style {
                    background: is_selected.then(|| theme::SELECTION.into()),
                    border: Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                });
            mouse_area(body)
                .on_press(Message::ExplorerSelect(entry.path.clone()))
                .on_double_click(Message::ExplorerActivate(entry.path.clone(), entry.is_dir))
                .into()
        }
    }
}

fn footer(name: &str, is_dir: bool) -> Element<'_, Message> {
    let action = |label: &'static str, id: CommandId| {
        button(text(label).size(12))
            .on_press(Message::Command(id))
            .padding([4, 8])
            .style(|_, status| {
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
                    text_color: if hovered { theme::CYAN } else { theme::TEXT },
                    border: Border {
                        color: theme::BORDER,
                        width: 1.0,
                        radius: 5.0.into(),
                    },
                    ..Default::default()
                }
            })
    };
    container(
        column![
            text(name)
                .size(12)
                .color(theme::TEXT)
                .wrapping(text::Wrapping::None),
            row![
                action("Insert path", CommandId::InsertSelectedPath),
                action("Copy", CommandId::CopySelectedPath),
                action(
                    if is_dir {
                        "Open tab here"
                    } else {
                        "Tab in folder"
                    },
                    CommandId::NewTabInSelectedFolder
                ),
            ]
            .spacing(4),
        ]
        .spacing(6),
    )
    .padding([8, 10])
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

fn icon_button<'a>(
    glyph: &'a str,
    tip: &'a str,
    id: CommandId,
    enabled: bool,
    active: bool,
) -> Element<'a, Message> {
    let color = match (enabled, active) {
        (false, _) => theme::DISABLED,
        (true, true) => theme::CYAN,
        (true, false) => theme::TEXT,
    };
    let b = button(container(text(glyph).size(13).color(color)).center_x(14))
        .padding([2, 5])
        .on_press_maybe(enabled.then_some(Message::Command(id)))
        .style(move |_, status| button::Style {
            background: (active
                || matches!(status, button::Status::Hovered | button::Status::Pressed))
            .then(|| theme::ELEVATED.into()),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });
    tooltip(
        b,
        container(text(tip).size(11).color(theme::TEXT))
            .padding([3, 6])
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
    )
    .into()
}

fn indent(depth: usize) -> f32 {
    depth as f32 * INDENT
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}
