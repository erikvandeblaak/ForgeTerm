//! Prompt Studio: pick a template, fill in its variables, preview, and insert the
//! result at the focused terminal's prompt (never followed by Enter) or copy it.
//! Shown as a floating window (modal) or as a panel beside the terminal (docked),
//! where the terminal keeps working and the result goes to whichever terminal has
//! the focus when Insert is pressed.

use std::collections::HashMap;

use forgeterm_prompts::{AUTO_VARIABLES, Library, Rendered, Template};
use iced::widget::{
    button, column, container, mouse_area, opaque, row, scrollable, space, text, text_editor,
};
use iced::{Border, Color, Element, Length, Shadow, Vector};

use crate::app::Message;
use crate::commands::CommandId;
use crate::terminal_view::TERMINAL_FONT;
use crate::theme;

pub struct PromptStudio {
    /// `None` while the templates are being read.
    library: Option<Library>,
    selected: usize,
    /// One editor per variable name, shared between templates (so `file` stays
    /// filled in when switching).
    editors: HashMap<String, text_editor::Content>,
}

impl PromptStudio {
    /// `auto` holds the values ForgeTerm knows (project, cwd, selection, …).
    pub fn new(auto: HashMap<String, String>) -> Self {
        let editors = auto
            .into_iter()
            .filter(|(_, v)| !v.trim().is_empty())
            .map(|(k, v)| (k, text_editor::Content::with_text(&v)))
            .collect();
        Self {
            library: None,
            selected: 0,
            editors,
        }
    }

    pub fn set_library(&mut self, library: Library) {
        // Keep the same template selected across a reload when it still exists.
        let current = self.current().map(|t| t.id.clone());
        self.selected = current
            .and_then(|id| library.templates.iter().position(|t| t.id == id))
            .unwrap_or(0);
        self.library = Some(library);
        self.ensure_editors();
    }

    /// Every variable of the current template has an editor (the view borrows them).
    fn ensure_editors(&mut self) {
        let names = self.current().map(Template::variables).unwrap_or_default();
        for name in names {
            self.editors.entry(name).or_default();
        }
    }

    pub fn current(&self) -> Option<&Template> {
        self.library.as_ref()?.templates.get(self.selected)
    }

    pub fn select(&mut self, index: usize) {
        if self
            .library
            .as_ref()
            .is_some_and(|l| index < l.templates.len())
        {
            self.selected = index;
            self.ensure_editors();
        }
    }

    pub fn edit(&mut self, name: String, action: text_editor::Action) {
        self.editors.entry(name).or_default().perform(action);
    }

    fn values(&self) -> HashMap<String, String> {
        self.editors
            .iter()
            .map(|(k, c)| {
                (
                    k.clone(),
                    c.text().trim_end_matches(['\r', '\n']).to_owned(),
                )
            })
            .collect()
    }

    pub fn rendered(&self) -> Option<Rendered> {
        self.current()
            .map(|t| forgeterm_prompts::render(&t.body, &self.values()))
    }

    /// Floating window. `target`: name of the terminal Insert goes to.
    pub fn view(&self, target: Option<&str>) -> Element<'_, Message> {
        let header = row![
            text("Prompt Studio").size(16).color(theme::TEXT),
            space::horizontal(),
            text(format!("Inserts into: {}", target.unwrap_or("no terminal")))
                .size(12)
                .color(theme::TEXT_MUTED),
            plain_button("Dock beside terminal", Some(DOCK)),
            plain_button("×", Some(Message::PromptClose)),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center);

        let body: Element<'_, Message> = match &self.library {
            None => container(text("Loading templates…").size(13).color(theme::TEXT_MUTED))
                .center(Length::Fill)
                .into(),
            Some(lib) => row![
                self.view_list(lib, Length::Fixed(270.0)),
                self.view_editor(false)
            ]
            .spacing(14)
            .height(Length::Fill)
            .into(),
        };

        let card =
            container(column![header, body, self.view_footer(target.is_some(), false)].spacing(12))
                .padding(16)
                .width(Length::Fixed(940.0))
                .height(Length::Fixed(620.0))
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
            .padding(16)
            .style(|_| container::Style {
                background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.35).into()),
                ..Default::default()
            });
        // Clicks outside the card do nothing (typed values would be lost otherwise).
        opaque(mouse_area(backdrop))
    }

    /// Panel beside the terminal. `focused`: the keyboard is in the studio (Esc or a
    /// click in a terminal gives it back).
    pub fn view_docked(&self, target: Option<&str>, focused: bool) -> Element<'_, Message> {
        let header = row![
            text("Prompt Studio").size(14).color(theme::TEXT),
            space::horizontal(),
            plain_button("Float", Some(DOCK)),
            plain_button("×", Some(Message::PromptClose)),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center);
        let target_line = text(format!(
            "Inserts into: {}{}",
            target.unwrap_or("no terminal"),
            if focused {
                "  ·  Esc returns to the terminal"
            } else {
                ""
            }
        ))
        .size(11)
        .color(theme::TEXT_MUTED);

        let body: Element<'_, Message> = match &self.library {
            None => container(text("Loading templates…").size(13).color(theme::TEXT_MUTED))
                .center(Length::Fill)
                .into(),
            Some(lib) => column![
                container(self.view_list(lib, Length::Fill)).height(Length::FillPortion(2)),
                container(self.view_editor(true)).height(Length::FillPortion(5)),
            ]
            .spacing(10)
            .height(Length::Fill)
            .into(),
        };

        container(
            column![
                header,
                target_line,
                body,
                self.view_footer(target.is_some(), true)
            ]
            .spacing(8),
        )
        .padding(12)
        .width(Length::Fixed(400.0))
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(theme::SURFACE.into()),
            border: Border {
                color: if focused { theme::CYAN } else { theme::BORDER },
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
    }

    fn view_list<'a>(&'a self, lib: &'a Library, width: Length) -> Element<'a, Message> {
        let mut list = column![].spacing(2);
        for (index, t) in lib.templates.iter().enumerate() {
            let selected = index == self.selected;
            let (tag, tag_color) = if t.is_builtin() {
                ("built-in", theme::TEXT_MUTED)
            } else {
                ("yours", theme::MINT)
            };
            let content = row![
                text(t.name.as_str()).size(13).color(theme::TEXT),
                space::horizontal(),
                text(tag).size(10).color(tag_color),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center);
            list = list.push(
                mouse_area(
                    container(content)
                        .padding([7, 10])
                        .width(Length::Fill)
                        .style(move |_| selectable(selected)),
                )
                .on_press(Message::PromptSelect(index)),
            );
        }
        let mut side = column![scrollable(list).height(Length::Fill),].spacing(8);
        for p in lib.problems.iter().take(3) {
            side = side.push(text(p.as_str()).size(11).color(theme::AMBER));
        }
        side = side.push(
            row![
                plain_button(
                    "Open prompts folder",
                    Some(Message::Command(CommandId::OpenPromptsFolder))
                ),
                plain_button("↻ Reload", Some(Message::PromptReload)),
            ]
            .spacing(6),
        );
        side = side.push(
            text("Your own templates: .md files in that folder, with {{variables}}.")
                .size(11)
                .color(theme::TEXT_MUTED),
        );
        container(side).width(width).into()
    }

    /// `docked`: the narrow panel gives the fields more room than the preview.
    fn view_editor(&self, docked: bool) -> Element<'_, Message> {
        let Some(t) = self.current() else {
            return container(text("No templates").size(13).color(theme::TEXT_MUTED))
                .center(Length::Fill)
                .into();
        };
        let mut fields = column![].spacing(8);
        if let Some(d) = &t.description {
            fields = fields.push(text(d.as_str()).size(12).color(theme::TEXT_MUTED));
        }
        for name in t.variables() {
            let auto = AUTO_VARIABLES.contains(&name.as_str());
            let label = row![
                text(name.clone()).size(12).color(theme::CYAN),
                text(if auto {
                    "filled in by ForgeTerm when known, editable"
                } else {
                    ""
                })
                .size(10)
                .color(theme::TEXT_MUTED),
            ]
            .spacing(8);
            if let Some(content) = self.editors.get(&name) {
                fields = fields.push(column![label, field(content, name.clone())].spacing(3));
            }
        }

        let preview_text = self.rendered().map(|r| r.text).unwrap_or_default();
        let preview = container(
            scrollable(
                text(preview_text)
                    .font(TERMINAL_FONT)
                    .size(12)
                    .color(theme::TEXT),
            )
            .height(Length::Fill)
            .width(Length::Fill),
        )
        .padding(10)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(theme::OBSIDIAN.into()),
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });

        column![
            scrollable(fields).height(Length::FillPortion(if docked { 3 } else { 1 })),
            text("PREVIEW").size(11).color(theme::TEXT_MUTED),
            container(preview).height(Length::FillPortion(if docked { 2 } else { 1 })),
        ]
        .spacing(8)
        .width(Length::Fill)
        .into()
    }

    /// `has_target`: a running terminal can take the text. `stacked`: status above
    /// the buttons (narrow panel).
    fn view_footer(&self, has_target: bool, stacked: bool) -> Element<'_, Message> {
        let rendered = self.rendered();
        let ready = rendered.as_ref().is_some_and(|r| r.missing.is_empty());
        let status = match &rendered {
            Some(r) if !r.missing.is_empty() => text(format!("Fill in: {}", r.missing.join(", ")))
                .size(12)
                .color(theme::AMBER),
            Some(_) if !has_target => text("No terminal to insert into; Copy still works.")
                .size(12)
                .color(theme::AMBER),
            Some(_) => text("Inserted at the prompt without Enter: you decide when to send it.")
                .size(12)
                .color(theme::TEXT_MUTED),
            None => text("").size(12),
        };
        let has_text = rendered.as_ref().is_some_and(|r| !r.text.is_empty());
        let buttons = row![
            space::horizontal(),
            plain_button("Copy", has_text.then_some(Message::PromptCopy)),
            primary_button(
                "Insert (no Enter)",
                (ready && has_target).then_some(Message::PromptInsert)
            ),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        if stacked {
            column![status, buttons].spacing(6).into()
        } else {
            row![status, buttons]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .into()
        }
    }
}

/// Header button: switch between floating window and panel beside the terminal.
const DOCK: Message = Message::Command(CommandId::TogglePromptStudioDock);

fn field(content: &text_editor::Content, name: String) -> Element<'_, Message> {
    text_editor(content)
        .placeholder("type a value…")
        .on_action(move |a| Message::PromptEdit(name.clone(), a))
        .size(13)
        .padding([6, 8])
        .min_height(30.0)
        .max_height(110.0)
        .style(editor_style)
        .into()
}

fn editor_style(_: &iced::Theme, status: text_editor::Status) -> text_editor::Style {
    let focused = matches!(status, text_editor::Status::Focused { .. });
    text_editor::Style {
        background: theme::GRAPHITE.into(),
        border: Border {
            color: if focused { theme::CYAN } else { theme::BORDER },
            width: 1.0,
            radius: 6.0.into(),
        },
        placeholder: theme::TEXT_MUTED,
        value: theme::TEXT,
        selection: theme::SELECTION,
    }
}

fn selectable(selected: bool) -> container::Style {
    container::Style {
        background: selected.then(|| theme::ELEVATED.into()),
        border: Border {
            color: if selected {
                theme::CYAN
            } else {
                Color::TRANSPARENT
            },
            width: if selected { 1.0 } else { 0.0 },
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

fn plain_button(label: &str, on_press: Option<Message>) -> Element<'_, Message> {
    let enabled = on_press.is_some();
    button(text(label).size(12))
        .on_press_maybe(on_press)
        .padding([6, 12])
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(
                    if hovered {
                        theme::ELEVATED
                    } else {
                        theme::GRAPHITE
                    }
                    .into(),
                ),
                text_color: if !enabled {
                    theme::DISABLED
                } else if hovered {
                    theme::CYAN
                } else {
                    theme::TEXT
                },
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

fn primary_button(label: &str, on_press: Option<Message>) -> Element<'_, Message> {
    let enabled = on_press.is_some();
    button(text(label).size(12))
        .on_press_maybe(on_press)
        .padding([6, 14])
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(
                    if !enabled {
                        theme::GRAPHITE
                    } else if hovered {
                        theme::MINT
                    } else {
                        theme::CYAN
                    }
                    .into(),
                ),
                text_color: if enabled {
                    theme::OBSIDIAN
                } else {
                    theme::DISABLED
                },
                border: Border {
                    color: if enabled { theme::CYAN } else { theme::BORDER },
                    width: 1.0,
                    radius: 5.0.into(),
                },
                ..Default::default()
            }
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn studio() -> PromptStudio {
        let auto = HashMap::from([
            ("project".to_owned(), "Forge".to_owned()),
            ("selection".to_owned(), "   ".to_owned()),
        ]);
        let mut s = PromptStudio::new(auto);
        s.set_library(forgeterm_prompts::load(None));
        s
    }

    #[test]
    fn blank_auto_values_are_not_prefilled() {
        let s = studio();
        assert_eq!(s.editors["project"].text().trim_end(), "Forge");
        // Blank auto value: the editor exists (template uses it) but starts empty.
        assert_eq!(s.current().unwrap().id, "explain-error");
        assert!(s.editors["selection"].text().trim().is_empty());
        assert_eq!(s.rendered().unwrap().missing, ["cwd", "selection"]);
    }

    #[test]
    fn typed_values_fill_the_template_and_are_shared() {
        let mut s = studio();
        let plan = s
            .library
            .as_ref()
            .unwrap()
            .templates
            .iter()
            .position(|t| t.id == "plan-feature")
            .unwrap();
        s.select(plan);
        assert_eq!(s.rendered().unwrap().missing, ["feature"]);
        for c in "Dark mode".chars() {
            s.edit(
                "feature".into(),
                text_editor::Action::Edit(text_editor::Edit::Insert(c)),
            );
        }
        let r = s.rendered().unwrap();
        assert!(r.missing.is_empty());
        assert!(r.text.contains("Dark mode"));
        assert!(r.text.contains("in Forge:"));
        // Selecting out of range keeps the current template.
        s.select(999);
        assert_eq!(s.current().unwrap().id, "plan-feature");
    }

    #[test]
    fn reload_keeps_the_selected_template() {
        let mut s = studio();
        s.select(3);
        let id = s.current().unwrap().id.clone();
        s.set_library(forgeterm_prompts::load(None));
        assert_eq!(s.current().unwrap().id, id);
    }
}
