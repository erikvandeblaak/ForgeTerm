//! Menu bar: structure (built from the command registry), navigation state and view.
//!
//! Menus contain only `CommandId`s, so every entry runs through `App::run_command`
//! exactly like a shortcut or palette entry. Entries that cannot do anything right now
//! are shown disabled (greyed out, not clickable) instead of silently doing nothing.

use iced::widget::{column, container, mouse_area, opaque, pin, row, space, stack, text};
use iced::{Border, Color, Element, Length, Shadow, Vector};

use crate::app::Message;
use crate::commands::{CommandId, Dir};
use crate::theme;

/// Height of the top bar; dropdowns open directly below it.
pub const BAR_HEIGHT: f32 = 40.0;
/// Horizontal position where the first menu title starts (bar padding + brand block).
pub const TITLES_X: f32 = 14.0 + BRAND_WIDTH + 8.0;
/// Width reserved for logo + product name in the top bar.
pub const BRAND_WIDTH: f32 = 112.0;
const DROPDOWN_WIDTH: f32 = 340.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Command(CommandId),
    Separator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    pub title: &'static str,
    pub items: Vec<Item>,
}

/// The menu structure. `shell_count` adds one "New Tab with …" entry per detected shell.
pub fn menus(shell_count: usize) -> Vec<Menu> {
    use CommandId::*;
    use Item::{Command as C, Separator as Sep};

    let mut file = vec![C(NewTab)];
    file.extend((0..shell_count).map(|i| C(NewTabWithShell(i))));
    file.extend([
        Sep,
        C(InsertSelectedPath),
        C(CopySelectedPath),
        C(NewTabInSelectedFolder),
        Sep,
        C(ClosePane),
        C(CloseTab),
        Sep,
        C(OpenSettings),
        C(ReloadSettings),
        Sep,
        C(Quit),
    ]);

    vec![
        Menu {
            title: "File",
            items: file,
        },
        Menu {
            title: "Edit",
            items: vec![C(Copy), C(Paste), Sep, C(ClearScrollback)],
        },
        Menu {
            title: "View",
            items: vec![
                C(CommandPalette),
                Sep,
                C(ToggleExplorer),
                C(ExplorerToggleHidden),
                C(ExplorerToggleFollow),
                C(ExplorerRefresh),
                C(ExplorerParent),
                C(ExplorerOpenSelected),
                Sep,
                C(ZoomIn),
                C(ZoomOut),
                C(ZoomReset),
                Sep,
                C(NextTab),
                C(PreviousTab),
            ],
        },
        Menu {
            title: "Terminal",
            items: vec![
                C(SplitRight),
                C(SplitDown),
                C(TogglePaneZoom),
                Sep,
                C(FocusPane(Dir::Left)),
                C(FocusPane(Dir::Right)),
                C(FocusPane(Dir::Up)),
                C(FocusPane(Dir::Down)),
                Sep,
                C(ScrollToTop),
                C(ScrollToBottom),
                C(ScrollPageUp),
                C(ScrollPageDown),
                Sep,
                C(RestartShell),
            ],
        },
        Menu {
            title: "AI",
            items: {
                let mut ai = vec![C(ToggleAiPanel), C(AiRefresh), Sep];
                for kind in forgeterm_agents::AgentKind::ALL {
                    ai.push(C(StartAgent(kind, false)));
                }
                for kind in forgeterm_agents::AgentKind::ALL {
                    ai.push(C(StartAgent(kind, true)));
                }
                ai.extend([Sep, C(OpenPromptStudio), C(OpenPromptsFolder)]);
                ai
            },
        },
        Menu {
            title: "Help",
            items: vec![C(About)],
        },
    ]
}

/// Fixed width of a menu title, so dropdown positions are known without measuring text.
pub fn title_width(title: &str) -> f32 {
    title.chars().count() as f32 * 7.5 + 22.0
}

/// Left edge of the dropdown of menu `index`.
pub fn dropdown_x(menus: &[Menu], index: usize) -> f32 {
    TITLES_X
        + menus
            .iter()
            .take(index)
            .map(|m| title_width(m.title))
            .sum::<f32>()
}

/// A menu entry resolved against the current app state (label, shortcut, enabled).
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    Command {
        id: CommandId,
        label: String,
        hint: Option<String>,
        enabled: bool,
    },
    Separator,
}

impl Row {
    fn enabled_command(&self) -> Option<CommandId> {
        match self {
            Row::Command {
                id, enabled: true, ..
            } => Some(*id),
            _ => None,
        }
    }
}

/// Which menu is open and which entry is highlighted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuState {
    pub open: usize,
    pub highlighted: Option<usize>,
}

impl MenuState {
    pub fn new(open: usize) -> Self {
        Self {
            open,
            highlighted: None,
        }
    }

    /// Opens the neighbouring menu (wrapping).
    pub fn switch(&mut self, menu_count: usize, delta: i32) {
        if menu_count == 0 {
            return;
        }
        let n = menu_count as i64;
        self.open = (self.open as i64 + i64::from(delta)).rem_euclid(n) as usize;
        self.highlighted = None;
    }

    /// Moves the highlight to the next enabled entry in `delta`'s direction, wrapping
    /// and skipping separators and disabled entries.
    pub fn step(&mut self, rows: &[Row], delta: i32) {
        let n = rows.len();
        if n == 0 || !rows.iter().any(|r| r.enabled_command().is_some()) {
            self.highlighted = None;
            return;
        }
        let forward = delta >= 0;
        let mut i = match (self.highlighted, forward) {
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
            (None, true) => 0,
            (None, false) => n - 1,
        };
        while rows[i].enabled_command().is_none() {
            i = if forward {
                (i + 1) % n
            } else {
                (i + n - 1) % n
            };
        }
        self.highlighted = Some(i);
    }

    /// Command under the highlight, if it is enabled.
    pub fn highlighted_command(&self, rows: &[Row]) -> Option<CommandId> {
        self.highlighted
            .and_then(|i| rows.get(i))
            .and_then(Row::enabled_command)
    }
}

// ---- view -------------------------------------------------------------------------

/// The row of menu titles shown in the top bar.
pub fn view_titles<'a>(menus: &[Menu], state: Option<&MenuState>) -> Element<'a, Message> {
    let mut titles = row![].align_y(iced::Alignment::Center);
    for (index, menu) in menus.iter().enumerate() {
        let open = state.is_some_and(|s| s.open == index);
        let label = container(text(menu.title).size(13).color(if open {
            theme::CYAN
        } else {
            theme::TEXT
        }))
        .center_x(Length::Fixed(title_width(menu.title)))
        .padding([5, 0])
        .style(move |_| container::Style {
            background: open.then(|| theme::ELEVATED.into()),
            border: Border {
                radius: 5.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });
        titles = titles.push(
            mouse_area(label)
                .on_press(Message::MenuOpen(index))
                .on_enter(Message::MenuHoverTitle(index))
                .interaction(iced::mouse::Interaction::Pointer),
        );
    }
    titles.into()
}

/// Overlay for an open menu: an invisible backdrop below the top bar (click = close)
/// and the dropdown itself. The top bar stays clickable so another title can be chosen.
pub fn view_dropdown<'a>(x: f32, rows: &[Row], state: &MenuState) -> Element<'a, Message> {
    let mut list = column![].spacing(1);
    for (index, r) in rows.iter().enumerate() {
        list = list.push(view_row(index, r, state.highlighted == Some(index)));
    }
    let card = container(list)
        .padding(5)
        .width(Length::Fixed(DROPDOWN_WIDTH))
        .style(|_| container::Style {
            background: Some(theme::SURFACE.into()),
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 8.0.into(),
            },
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
                offset: Vector::new(0.0, 8.0),
                blur_radius: 24.0,
            },
            ..Default::default()
        });

    let backdrop = column![
        space::vertical().height(BAR_HEIGHT),
        opaque(
            mouse_area(space().width(Length::Fill).height(Length::Fill))
                .on_press(Message::MenuClose)
        ),
    ];
    stack![backdrop, pin(opaque(card)).x(x).y(BAR_HEIGHT - 2.0)].into()
}

fn view_row<'a>(index: usize, r: &Row, highlighted: bool) -> Element<'a, Message> {
    match r {
        Row::Separator => container(container(space().height(1).width(Length::Fill)).style(|_| {
            container::Style {
                background: Some(theme::BORDER.into()),
                ..Default::default()
            }
        }))
        .padding([4, 8])
        .into(),
        Row::Command {
            label,
            hint,
            enabled,
            ..
        } => {
            let (fg, hint_fg) = if *enabled {
                (theme::TEXT, theme::TEXT_MUTED)
            } else {
                (theme::DISABLED, theme::DISABLED)
            };
            let content = row![
                text(label.clone()).size(13).color(fg),
                space::horizontal(),
                text(hint.clone().unwrap_or_default())
                    .size(11)
                    .color(hint_fg),
            ]
            .spacing(16)
            .align_y(iced::Alignment::Center);
            let highlighted = highlighted && *enabled;
            let body = container(content)
                .padding([6, 10])
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: highlighted.then(|| theme::ELEVATED.into()),
                    border: Border {
                        color: if highlighted {
                            theme::CYAN
                        } else {
                            Color::TRANSPARENT
                        },
                        width: if highlighted { 1.0 } else { 0.0 },
                        radius: 5.0.into(),
                    },
                    ..Default::default()
                });
            let area = mouse_area(body).on_enter(Message::MenuHoverItem(index));
            if *enabled {
                area.on_press(Message::MenuRun(index))
                    .interaction(iced::mouse::Interaction::Pointer)
                    .into()
            } else {
                area.into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::STATIC_COMMANDS;

    fn cmd(id: CommandId, enabled: bool) -> Row {
        Row::Command {
            id,
            label: id.static_title().to_owned(),
            hint: None,
            enabled,
        }
    }

    #[test]
    fn every_static_command_is_in_a_menu() {
        let all: Vec<CommandId> = menus(2)
            .iter()
            .flat_map(|m| m.items.iter())
            .filter_map(|i| match i {
                Item::Command(id) => Some(*id),
                Item::Separator => None,
            })
            .collect();
        for id in STATIC_COMMANDS {
            assert!(all.contains(id), "{id:?} missing from the menus");
        }
        assert!(all.contains(&CommandId::NewTabWithShell(0)));
        assert!(all.contains(&CommandId::NewTabWithShell(1)));
        assert!(!all.contains(&CommandId::NewTabWithShell(2)));
    }

    #[test]
    fn separators_only_between_groups() {
        for menu in menus(3) {
            let items = &menu.items;
            assert_ne!(items.first(), Some(&Item::Separator), "{}", menu.title);
            assert_ne!(items.last(), Some(&Item::Separator), "{}", menu.title);
            for pair in items.windows(2) {
                assert!(
                    !(pair[0] == Item::Separator && pair[1] == Item::Separator),
                    "double separator in {}",
                    menu.title
                );
            }
        }
    }

    #[test]
    fn dropdown_positions_follow_title_widths() {
        let m = menus(0);
        assert_eq!(dropdown_x(&m, 0), TITLES_X);
        assert_eq!(dropdown_x(&m, 1), TITLES_X + title_width("File"));
        assert!(dropdown_x(&m, 3) > dropdown_x(&m, 2));
    }

    #[test]
    fn step_skips_separators_and_disabled_entries_and_wraps() {
        use CommandId::*;
        let rows = vec![
            cmd(Copy, false),
            cmd(Paste, true),
            Row::Separator,
            cmd(ClearScrollback, true),
        ];
        let mut s = MenuState::new(1);
        s.step(&rows, 1);
        assert_eq!(s.highlighted, Some(1));
        s.step(&rows, 1);
        assert_eq!(s.highlighted, Some(3));
        s.step(&rows, 1);
        assert_eq!(
            s.highlighted,
            Some(1),
            "wraps past the disabled first entry"
        );
        s.step(&rows, -1);
        assert_eq!(s.highlighted, Some(3));
        assert_eq!(s.highlighted_command(&rows), Some(ClearScrollback));

        let mut up = MenuState::new(1);
        up.step(&rows, -1);
        assert_eq!(up.highlighted, Some(3));
    }

    #[test]
    fn nothing_enabled_means_no_highlight() {
        let rows = vec![cmd(CommandId::Copy, false), Row::Separator];
        let mut s = MenuState::new(0);
        s.step(&rows, 1);
        assert_eq!(s.highlighted, None);
        assert_eq!(s.highlighted_command(&rows), None);
        // A disabled entry under the mouse never runs.
        s.highlighted = Some(0);
        assert_eq!(s.highlighted_command(&rows), None);
    }

    #[test]
    fn switch_wraps_and_clears_highlight() {
        let mut s = MenuState {
            open: 0,
            highlighted: Some(2),
        };
        s.switch(5, -1);
        assert_eq!(s, MenuState::new(4));
        s.switch(5, 1);
        assert_eq!(s.open, 0);
    }
}
