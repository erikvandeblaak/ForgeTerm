//! Explorer side panel (view only; state lives in `forgeterm_explorer::Tree`).

use std::path::Path;

use forgeterm_explorer::{Row, Tree};
use iced::widget::{
    button, column, container, mouse_area, row, scrollable, space, text, text_input, tooltip,
};
use iced::{Border, Element, Length};

use crate::app::Message;
use crate::commands::CommandId;
use crate::theme;

pub const WIDTH: f32 = 270.0;
const INDENT: f32 = 14.0;

/// `focused`: the explorer has the keyboard (selection drawn with a cyan edge).
/// `goto`: the "Go to Folder" field's text and problem, when open.
pub fn view<'a>(
    tree: &'a Tree,
    follow: bool,
    focused: bool,
    goto: Option<&'a (String, Option<String>)>,
) -> Element<'a, Message> {
    let root_name = folder_name(tree.root());
    let header = row![
        text("EXPLORER")
            .size(theme::size::TINY)
            .color(theme::text_muted()),
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
            "…",
            "Go to Folder…",
            CommandId::ExplorerGoToFolder,
            true,
            goto.is_some()
        ),
        icon_button(
            ".*",
            "Show Hidden Files",
            CommandId::ExplorerToggleHidden,
            true,
            tree.show_hidden()
        ),
    ]
    .spacing(theme::space::XXS)
    .align_y(iced::Alignment::Center);

    let root = column![
        text(root_name).size(theme::size::BODY).color(theme::text()),
        // A long path is cut at the panel edge instead of running over the terminal.
        container(
            text(tree.root().to_string_lossy().into_owned())
                .size(theme::size::TINY)
                .color(theme::text_muted())
                .wrapping(text::Wrapping::None)
        )
        .width(Length::Fill)
        .clip(true),
    ]
    .spacing(1);

    let selected = tree.selected().map(|e| e.path.as_path());
    let mut list = column![].spacing(0);
    for r in tree.rows() {
        list = list.push(view_row(&r, selected, focused));
    }
    let list = scrollable(container(list).padding([0, 4]))
        .id(crate::app::EXPLORER_LIST_ID)
        .on_scroll(Message::ExplorerScrolled)
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new().width(6).scroller_width(6),
        ))
        .height(Length::Fill)
        .width(Length::Fill);

    let mut top = column![header, root].spacing(theme::space::S);
    if let Some((value, problem)) = goto {
        top = top.push(
            text_input("Folder path (~ = home, relative = from here)", value)
                .id(crate::app::EXPLORER_GOTO_ID)
                .on_input(Message::ExplorerGoToInput)
                .on_submit(Message::ExplorerGoToSubmit)
                .size(theme::size::SMALL)
                .padding([4, 6]),
        );
        let hint = problem
            .clone()
            .unwrap_or_else(|| "Enter opens the folder, Esc cancels.".into());
        let color = if problem.is_some() {
            theme::coral()
        } else {
            theme::text_muted()
        };
        top = top.push(text(hint).size(theme::size::TINY).color(color));
    }
    let mut panel = column![container(top).padding([8, 10]), list].spacing(theme::space::XXS);
    if let Some(entry) = tree.selected() {
        panel = panel.push(footer(&entry.name, entry.is_dir));
    }

    container(panel)
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

fn view_row<'a>(r: &Row<'a>, selected: Option<&Path>, focused: bool) -> Element<'a, Message> {
    match r {
        Row::Note { depth, text: note } => row![
            space::horizontal().width(indent(*depth) + 16.0),
            text(note.clone())
                .size(theme::size::SMALL)
                .color(theme::text_muted()),
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
                theme::text_muted()
            } else {
                theme::text()
            };
            let chevron: Element<'a, Message> = if entry.is_dir {
                mouse_area(
                    container(
                        text(if *expanded { "▾" } else { "▸" })
                            .size(theme::size::TITLE)
                            .color(theme::text_muted()),
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
                    .size(theme::size::BODY)
                    .color(color)
                    .wrapping(text::Wrapping::None),
            ]
            .align_y(iced::Alignment::Center);
            let body = container(label)
                .height(22)
                .width(Length::Fill)
                .align_y(iced::Alignment::Center)
                .style(move |_| container::Style {
                    background: is_selected.then(|| theme::selection().into()),
                    border: Border {
                        radius: theme::radius::S.into(),
                        // The cyan edge shows that arrow keys move this selection.
                        color: theme::cyan(),
                        width: if is_selected && focused { 1.0 } else { 0.0 },
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
        button(text(label).size(theme::size::SMALL))
            .on_press(Message::Command(id))
            .padding([4, 8])
            .style(|_, status| {
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
    };
    container(
        column![
            container(
                text(name)
                    .size(theme::size::SMALL)
                    .color(theme::text())
                    .wrapping(text::Wrapping::None)
            )
            .width(Length::Fill)
            .clip(true),
            row![
                action("Insert path", CommandId::InsertSelectedPath),
                action("Copy", CommandId::CopySelectedPath),
            ]
            .spacing(theme::space::XS),
            if is_dir {
                row![action("Open tab here", CommandId::NewTabInSelectedFolder)]
            } else {
                row![
                    action("View", CommandId::ViewSelectedFile),
                    action("Tab in folder", CommandId::NewTabInSelectedFolder),
                ]
            }
            .spacing(theme::space::XS),
        ]
        .spacing(theme::space::S),
    )
    .padding([8, 10])
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

fn icon_button<'a>(
    glyph: &'a str,
    tip: &'a str,
    id: CommandId,
    enabled: bool,
    active: bool,
) -> Element<'a, Message> {
    let color = match (enabled, active) {
        (false, _) => theme::disabled(),
        (true, true) => theme::cyan(),
        (true, false) => theme::text(),
    };
    let b = button(container(text(glyph).size(theme::size::BODY).color(color)).center_x(14))
        .padding([2, 5])
        .on_press_maybe(enabled.then_some(Message::Command(id)))
        .style(move |_, status| button::Style {
            background: (active
                || matches!(status, button::Status::Hovered | button::Status::Pressed))
            .then(|| theme::elevated().into()),
            border: Border {
                radius: theme::radius::S.into(),
                ..Default::default()
            },
            ..Default::default()
        });
    tooltip(
        b,
        container(text(tip).size(theme::size::TINY).color(theme::text()))
            .padding([3, 6])
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
