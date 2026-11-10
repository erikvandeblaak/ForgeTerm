//! Command Palette: state, filtering and view.

use iced::widget::{
    Id, column, container, mouse_area, opaque, rich_text, row, space, span, text, text_input,
};
use iced::{Border, Element, Length, Shadow, Vector};

use crate::app::Message;
use crate::commands::CommandId;
use crate::fuzzy;
use crate::theme;

pub const INPUT_ID: &str = "forgeterm-command-palette";
/// Rows shown at once; the window follows the selection.
const VISIBLE_ROWS: usize = 10;
/// How many recently run commands are remembered for the palette.
pub const RECENT_LIMIT: usize = 5;

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: CommandId,
    /// "Category: Title" — what is searched and shown.
    pub label: String,
    /// Key binding text, if any.
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub entry: Entry,
    /// Char indices of `entry.label` matched by the query (for highlighting).
    pub highlight: Vec<usize>,
}

pub struct CommandPalette {
    entries: Vec<Entry>,
    recent: Vec<CommandId>,
    query: String,
    results: Vec<Match>,
    selected: usize,
    offset: usize,
}

impl CommandPalette {
    /// `entries` in default order; `recent` most-recent first.
    pub fn new(entries: Vec<Entry>, recent: &[CommandId]) -> Self {
        let mut palette = Self {
            entries,
            recent: recent.to_vec(),
            query: String::new(),
            results: Vec::new(),
            selected: 0,
            offset: 0,
        };
        palette.filter();
        palette
    }

    /// Appends typed text (used for keys that arrive before the input has focus).
    pub fn push_query(&mut self, text: &str) {
        let mut query = std::mem::take(&mut self.query);
        query.push_str(text);
        self.set_query(query);
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
        self.filter();
    }

    fn filter(&mut self) {
        self.selected = 0;
        self.offset = 0;
        if self.query.trim().is_empty() {
            // Recently used first, then everything else in registry order.
            let rank = |e: &Entry| {
                self.recent
                    .iter()
                    .position(|r| *r == e.id)
                    .unwrap_or(usize::MAX)
            };
            let mut entries = self.entries.clone();
            entries.sort_by_key(rank); // stable: keeps registry order for the rest
            self.results = entries
                .into_iter()
                .map(|entry| Match {
                    entry,
                    highlight: Vec::new(),
                })
                .collect();
            return;
        }
        let mut scored: Vec<(i32, usize, Match)> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(order, entry)| {
                fuzzy::score(&self.query, &entry.label).map(|(score, highlight)| {
                    (
                        score,
                        order,
                        Match {
                            entry: entry.clone(),
                            highlight,
                        },
                    )
                })
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        self.results = scored.into_iter().map(|(_, _, m)| m).collect();
    }

    #[cfg(test)]
    pub fn results(&self) -> &[Match] {
        &self.results
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    pub fn selected(&self) -> Option<CommandId> {
        self.results.get(self.selected).map(|m| m.entry.id)
    }

    pub fn command_at(&self, index: usize) -> Option<CommandId> {
        self.results.get(index).map(|m| m.entry.id)
    }

    /// Moves the selection, wrapping around, and keeps it inside the visible window.
    pub fn move_selection(&mut self, delta: i32) {
        let n = self.results.len();
        if n == 0 {
            return;
        }
        let n_i = n as i64;
        self.selected = (self.selected as i64 + i64::from(delta)).rem_euclid(n_i) as usize;
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + VISIBLE_ROWS {
            self.offset = self.selected + 1 - VISIBLE_ROWS;
        }
    }

    pub fn visible_range(&self) -> std::ops::Range<usize> {
        self.offset..(self.offset + VISIBLE_ROWS).min(self.results.len())
    }

    pub fn view(&self) -> Element<'_, Message> {
        let input = text_input("Type a command…", &self.query)
            .id(Id::new(INPUT_ID))
            .on_input(Message::PaletteQuery)
            .on_submit(Message::PaletteRun(None))
            .size(14)
            .padding([10, 12])
            .style(|_, _| text_input::Style {
                background: theme::GRAPHITE.into(),
                border: Border {
                    color: theme::CYAN,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                icon: theme::TEXT_MUTED,
                placeholder: theme::TEXT_MUTED,
                value: theme::TEXT,
                selection: theme::SELECTION,
            });

        let mut list = column![].spacing(2);
        for index in self.visible_range() {
            list = list.push(self.view_row(index));
        }
        let footer_text = match self.results.len() {
            0 => "No matching commands".to_owned(),
            n if n > VISIBLE_ROWS => {
                format!(
                    "{} of {n} · ↑↓ select · Enter run · Esc close",
                    self.selected + 1
                )
            }
            _ => "↑↓ select · Enter run · Esc close".to_owned(),
        };

        let card = container(
            column![
                input,
                list,
                text(footer_text).size(11).color(theme::TEXT_MUTED)
            ]
            .spacing(8),
        )
        .padding(10)
        .width(Length::Fixed(600.0))
        .style(|_| container::Style {
            background: Some(theme::SURFACE.into()),
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 10.0.into(),
            },
            shadow: Shadow {
                color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.55),
                offset: Vector::new(0.0, 12.0),
                blur_radius: 32.0,
            },
            ..Default::default()
        });

        // Backdrop: dims the app and closes the palette on click outside the card.
        // `opaque` keeps clicks on the card (and everything else) from reaching the
        // terminals underneath.
        let backdrop = container(column![space::vertical().height(70), opaque(card)])
            .center_x(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.35).into()),
                ..Default::default()
            });
        opaque(mouse_area(backdrop).on_press(Message::PaletteClose))
    }

    fn view_row(&self, index: usize) -> Element<'_, Message> {
        let m = &self.results[index];
        let selected = index == self.selected;

        // Group consecutive chars by highlight state into spans.
        let mut spans = Vec::new();
        let mut current = String::new();
        let mut current_hl = false;
        for (i, ch) in m.entry.label.chars().enumerate() {
            let hl = m.highlight.contains(&i);
            if hl != current_hl && !current.is_empty() {
                spans.push(styled_span(std::mem::take(&mut current), current_hl));
            }
            current_hl = hl;
            current.push(ch);
        }
        if !current.is_empty() {
            spans.push(styled_span(current, current_hl));
        }

        let hint = text(m.entry.hint.clone().unwrap_or_default())
            .size(11)
            .color(theme::TEXT_MUTED);
        let content = row![rich_text(spans).size(13), space::horizontal(), hint]
            .align_y(iced::Alignment::Center);

        mouse_area(
            container(content)
                .padding([7, 10])
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: selected.then(|| theme::ELEVATED.into()),
                    border: Border {
                        color: if selected {
                            theme::CYAN
                        } else {
                            iced::Color::TRANSPARENT
                        },
                        width: if selected { 1.0 } else { 0.0 },
                        radius: 6.0.into(),
                    },
                    ..Default::default()
                }),
        )
        .on_press(Message::PaletteRun(Some(index)))
        .on_enter(Message::PaletteHover(index))
        .into()
    }
}

fn styled_span<'a>(s: String, highlighted: bool) -> iced::widget::text::Span<'a, Message> {
    span(s).color(if highlighted {
        theme::CYAN
    } else {
        theme::TEXT
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries() -> Vec<Entry> {
        [
            (CommandId::NewTab, "Tab: New Tab"),
            (CommandId::SplitRight, "Pane: Split Right"),
            (CommandId::SplitDown, "Pane: Split Down"),
            (CommandId::ScrollPageUp, "Terminal: Scroll Page Up"),
            (CommandId::ZoomIn, "View: Increase Font Size"),
        ]
        .into_iter()
        .map(|(id, label)| Entry {
            id,
            label: label.into(),
            hint: None,
        })
        .collect()
    }

    #[test]
    fn empty_query_lists_recent_first_then_registry_order() {
        let p = CommandPalette::new(entries(), &[CommandId::ZoomIn, CommandId::SplitDown]);
        let ids: Vec<_> = p.results().iter().map(|m| m.entry.id).collect();
        assert_eq!(
            ids,
            vec![
                CommandId::ZoomIn,
                CommandId::SplitDown,
                CommandId::NewTab,
                CommandId::SplitRight,
                CommandId::ScrollPageUp
            ]
        );
    }

    #[test]
    fn query_filters_and_ranks() {
        let mut p = CommandPalette::new(entries(), &[]);
        p.set_query("split".into());
        let ids: Vec<_> = p.results().iter().map(|m| m.entry.id).collect();
        assert_eq!(ids, vec![CommandId::SplitRight, CommandId::SplitDown]);
        assert_eq!(p.selected(), Some(CommandId::SplitRight));

        p.set_query("spr".into());
        assert_eq!(p.selected(), Some(CommandId::SplitRight));

        p.set_query("qqq".into());
        assert!(p.results().is_empty());
        assert_eq!(p.selected(), None);
    }

    #[test]
    fn early_keystrokes_extend_the_query() {
        let mut p = CommandPalette::new(entries(), &[]);
        for ch in ["s", "p", "l"] {
            p.push_query(ch);
        }
        assert_eq!(p.selected(), Some(CommandId::SplitRight));
        assert_eq!(p.results().len(), 2);
    }

    #[test]
    fn selection_wraps_and_resets_on_new_query() {
        let mut p = CommandPalette::new(entries(), &[]);
        p.move_selection(-1);
        assert_eq!(p.selected(), Some(CommandId::ZoomIn));
        p.move_selection(1);
        assert_eq!(p.selected(), Some(CommandId::NewTab));
        p.move_selection(2);
        p.set_query("pane".into());
        assert_eq!(p.selected_index(), 0);
    }

    #[test]
    fn visible_window_follows_selection() {
        let many: Vec<Entry> = (0..25)
            .map(|i| Entry {
                id: CommandId::SelectTab(i),
                label: format!("Tab: Go to Tab {}", i + 1),
                hint: None,
            })
            .collect();
        let mut p = CommandPalette::new(many, &[]);
        assert_eq!(p.visible_range(), 0..VISIBLE_ROWS);
        p.move_selection(12);
        assert!(p.visible_range().contains(&12));
        p.move_selection(-12);
        assert_eq!(p.visible_range().start, 0);
        p.move_selection(-1); // wraps to the last entry
        assert_eq!(p.selected_index(), 24);
        assert_eq!(p.visible_range(), 15..25);
    }
}
