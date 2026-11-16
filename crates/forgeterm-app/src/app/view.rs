//! The window layout: top bar, panes, status bar, overlays.

use super::*;
use iced::widget::column;

impl App {
    /// Help → Keyboard Shortcuts: every command with its keys (from the live
    /// keymap, so [keys] changes show) and its name for settings.toml.
    pub(super) fn view_shortcuts(&self) -> Element<'_, Message> {
        let mut ids: Vec<CommandId> = STATIC_COMMANDS.to_vec();
        ids.push(CommandId::CommandPalette);
        ids.extend((0..self.shells.len().min(9)).map(CommandId::NewTabWithShell));
        for k in AgentKind::ALL {
            ids.push(CommandId::StartAgent(k, false));
            ids.push(CommandId::StartAgent(k, true));
        }
        ids.sort_by_key(|id| id.category());
        let mut list = column![].spacing(theme::space::XXS);
        let mut last_category = "";
        for id in ids {
            if id.category() != last_category {
                last_category = id.category();
                list = list.push(
                    container(
                        text(last_category)
                            .size(theme::size::TINY)
                            .color(theme::text_muted()),
                    )
                    .padding(iced::Padding {
                        top: 8.0,
                        right: 0.0,
                        bottom: 2.0,
                        left: 0.0,
                    }),
                );
            }
            let keys: Vec<String> = self
                .keymap
                .bindings_for(id)
                .iter()
                .map(ToString::to_string)
                .collect();
            list = list.push(
                row![
                    text(self.command_title(id))
                        .size(theme::size::BODY)
                        .color(theme::text())
                        .width(Length::FillPortion(5)),
                    text(if keys.is_empty() {
                        "—".to_owned()
                    } else {
                        keys.join("  ")
                    })
                    .size(theme::size::SMALL)
                    .color(if keys.is_empty() {
                        theme::disabled()
                    } else {
                        theme::cyan()
                    })
                    .width(Length::FillPortion(4)),
                    text(id.config_name())
                        .size(theme::size::TINY)
                        .font(crate::terminal_view::TERMINAL_FONT)
                        .color(theme::text_muted())
                        .width(Length::FillPortion(4)),
                ]
                .spacing(theme::space::L)
                .align_y(iced::Alignment::Center),
            );
        }
        let families = [
            self.keymap
                .range_hint(CommandId::SelectTab(0), CommandId::SelectTab(8))
                .map(|k| format!("{k} switch tabs")),
            self.keymap
                .range_hint(CommandId::NewTabWithShell(0), CommandId::NewTabWithShell(8))
                .map(|k| format!("{k} new tab with shell n")),
        ];
        let alt_n = text(
            families
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · "),
        )
        .size(theme::size::SMALL)
        .color(theme::text_muted());
        let card = container(
            column![
                row![
                    text("Keyboard Shortcuts").size(theme::size::HEADING).color(theme::text()),
                    space::horizontal(),
                    button(text("Close (Esc)").size(theme::size::SMALL))
                        .on_press(Message::ShortcutsClose)
                        .padding([5, 12])
                        .style(|_, status| ghost_button(status, theme::cyan())),
                ]
                .align_y(iced::Alignment::Center),
                text("Change them in settings.toml under [keys]: \"Ctrl+K\" = \"command_palette\", or \"none\" to remove one.")
                    .size(theme::size::SMALL)
                    .color(theme::text_muted()),
                alt_n,
                scrollable(list).height(Length::Fill),
            ]
            .spacing(theme::space::M),
        )
        .padding(18)
        .width(Length::Fixed(760.0))
        .height(Length::Fill)
        .max_height(680.0)
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
            .padding(24)
            .style(|_| container::Style {
                background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.35).into()),
                ..Default::default()
            });
        opaque(mouse_area(backdrop).on_press(Message::ShortcutsClose))
    }

    pub fn view(&self) -> Element<'_, Message> {
        let mut layout = column![self.view_top_bar()];
        if !self.settings_problems.is_empty() {
            layout = layout.push(settings_banner(&self.settings_problems));
        }
        if let Some((_, pending)) = &self.pending_paste {
            layout = layout.push(paste_banner(pending));
        }
        if let Some(n) = &self.notice {
            layout = layout.push(notice_banner(n));
        }
        if let Some(f) = &self.term_find {
            layout = layout.push(term_find_bar(f));
        }
        let body: Element<'_, Message> = match (&self.error, self.workspace.active_tab()) {
            (Some(err), _) => container(text(err.as_str()).color(theme::coral()))
                .center(Length::Fill)
                .into(),
            (None, _) if self.mission.visible => self.view_mission(),
            (None, _) if self.git_dash.visible => self.view_git_dash(),
            (None, _) if self.compare.visible => self.view_compare(),
            (None, Some(tab)) => self.view_panes(tab),
            (None, None) => space::vertical().into(),
        };
        let body: Element<'_, Message> = if self.panel_open(SidePanel::Explorer) {
            row![
                explorer_view::view(
                    &self.explorer,
                    self.explorer_follow,
                    self.explorer_focused,
                    self.explorer_goto.as_ref(),
                ),
                body
            ]
            .into()
        } else if self.panel_open(SidePanel::Ai) {
            row![ai_view::view(&self.agents, self.ai_context.as_ref()), body].into()
        } else if self.panel_open(SidePanel::Git) {
            row![git_view::view(&self.git), body].into()
        } else if self.panel_open(SidePanel::Monitor) {
            row![monitor_view::view(&self.monitor, self.monitor_rows()), body].into()
        } else {
            body
        };
        let body: Element<'_, Message> = if self.viewer.visible {
            row![
                container(body).width(Length::FillPortion(5)),
                container(viewer_view::view(&self.viewer, self.metrics.font_size))
                    .width(Length::FillPortion(4)),
            ]
            .into()
        } else {
            body
        };
        let target = self.prompt_target().map(|t| t.shell.name.as_str());
        let body: Element<'_, Message> = match &self.prompt_studio {
            Some(studio) if self.prompt_docked => {
                row![body, studio.view_docked(target, self.prompt_focused)].into()
            }
            _ => body,
        };
        let base: Element<'_, Message> = layout.push(body).push(self.view_status_bar()).into();
        let mut layers = stack![base];
        if let Some(state) = &self.menu {
            let x = menu::dropdown_x(&self.menus, state.open);
            layers = layers.push(menu::view_dropdown(x, &self.menu_rows(state.open), state));
        }
        if let Some(p) = &self.command_palette {
            layers = layers.push(p.view());
        }
        if let Some(h) = &self.git.history {
            let branch = self
                .git
                .status()
                .map_or_else(String::new, |s| s.branch.label());
            layers = layers.push(git_view::view_history(h, branch));
        }
        if let Some(diff) = &self.git.diff {
            layers = layers.push(git_view::view_diff(diff));
        }
        if let Some(studio) = self.prompt_studio.as_ref().filter(|_| !self.prompt_docked) {
            layers = layers.push(studio.view(target));
        }
        if self.about_open {
            layers = layers.push(view_about());
        }
        if self.shortcuts_open {
            layers = layers.push(self.view_shortcuts());
        }
        if let Some(form) = &self.worktree_form {
            layers = layers.push(worktree_form::view(form, &self.agents));
        }
        if let Some((_, path, branch)) = &self.pending_worktree_remove {
            layers = layers.push(worktree_form::view_remove(path, branch.as_deref()));
        }
        if let Some((target, programs)) = &self.pending_close {
            layers = layers.push(view_close_confirm(target, programs));
        }
        if self.drop_hover {
            let shell = self
                .focused()
                .map_or("the terminal", |t| t.shell.name.as_str());
            layers = layers.push(drop_hint(shell));
        }
        layers.into()
    }

    /// What a tab's terminals want from the user: a program's status (OSC 7501) or
    /// a bell that rang while the user was elsewhere. Finished results disappear once
    /// the tab has been looked at.
    pub(super) fn tab_badge(&self, tab: &crate::workspace::Tab) -> Option<(String, Color)> {
        use forgeterm_terminal::status::ProgramState;
        let terminals: Vec<&TerminalPane> = tab
            .terminals()
            .filter_map(|id| self.terminals.get(&id))
            .collect();
        let status = terminals
            .iter()
            .filter_map(|t| t.program_status())
            .max_by_key(|s| s.state);
        let bell = terminals.iter().any(|t| t.bell);
        match status.map(|s| (s.state, s.progress)) {
            Some((ProgramState::Blocked, _)) => Some(("needs you".into(), theme::amber())),
            Some((ProgramState::Error, _)) => Some(("failed".into(), theme::coral())),
            Some((ProgramState::Done, _)) => Some(("done".into(), theme::mint())),
            _ if bell => Some(("bell".into(), theme::amber())),
            Some((ProgramState::Working, Some(p))) => Some((format!("{p}%"), theme::cyan())),
            Some((ProgramState::Working, None)) => Some(("working".into(), theme::cyan())),
            _ => None,
        }
    }

    pub(super) fn view_top_bar(&self) -> Element<'_, Message> {
        let mut tabs = row![]
            .spacing(theme::space::XS)
            .align_y(iced::Alignment::Center);
        for (index, tab) in self.workspace.tabs.iter().enumerate() {
            let active = index == self.workspace.active;
            let focused = tab.focused().and_then(|id| self.terminals.get(&id));
            let title = focused.map_or_else(|| "—".to_owned(), TerminalPane::display_title);
            let dot = match focused.map(|t| &t.status) {
                Some(Status::Running) | None => theme::mint(),
                Some(Status::Exited(_)) => theme::amber(),
                Some(Status::Failed(_)) => theme::coral(),
            };
            let panes = tab.panes.len();
            let label = if panes > 1 {
                format!("{}  ·{panes}", truncate(&title, MAX_TAB_TITLE))
            } else {
                truncate(&title, MAX_TAB_TITLE)
            };
            let mut content = row![
                text("●").size(9).color(dot),
                text(label).size(theme::size::SMALL).color(if active {
                    theme::text()
                } else {
                    theme::text_muted()
                }),
            ]
            .spacing(7)
            .align_y(iced::Alignment::Center);
            if let Some((badge, color)) = self.tab_badge(tab) {
                content = content.push(text(badge).size(theme::size::TINY).color(color));
            }
            let select = button(content)
                .on_press(Message::SelectTab(index))
                .padding([5, 4])
                .style(|_, _| button::Style::default());
            let close = button(text("×").size(theme::size::BODY).color(theme::text_muted()))
                .on_press(Message::Command(CommandId::CloseTabAt(index)))
                .padding([1, 6])
                .style(|_, status| ghost_button(status, theme::coral()));
            tabs = tabs.push(
                container(
                    row![select, close]
                        .spacing(theme::space::XXS)
                        .align_y(iced::Alignment::Center),
                )
                .padding([0, 6])
                .style(move |_| tab_style(active)),
            );
        }

        let choices: Vec<ShellChoice> = self
            .shells
            .iter()
            .enumerate()
            .map(|(index, s)| ShellChoice {
                command: CommandId::NewTabWithShell(index),
                name: s.name.clone(),
            })
            .chain(
                self.ssh_hosts
                    .iter()
                    .enumerate()
                    .filter(|_| self.ssh_program.is_some())
                    .map(|(index, h)| ShellChoice {
                        command: CommandId::NewSshTab(index),
                        name: format!("SSH: {}", h.alias),
                    }),
            )
            .collect();
        let new_tab = button(text("+").size(theme::size::TITLE).color(theme::text()))
            .on_press(Message::Command(CommandId::NewTab))
            .padding([2, 9])
            .style(|_, status| ghost_button(status, theme::cyan()));
        let shell_menu = pick_list(choices, None::<ShellChoice>, Message::ShellPicked)
            .placeholder("New tab with…")
            .width(Length::Fixed(150.0))
            .text_size(12)
            .padding([3, 8]);

        let brand = container(
            row![
                svg(svg::Handle::from_memory(LOGO)).width(22).height(22),
                text("ForgeTerm")
                    .size(theme::size::TITLE)
                    .color(theme::text()),
            ]
            .spacing(theme::space::M)
            .align_y(iced::Alignment::Center),
        )
        .width(Length::Fixed(menu::BRAND_WIDTH));

        container(
            row![
                brand,
                menu::view_titles(&self.menus, self.menu.as_ref()),
                space::horizontal().width(10),
                // Many tabs scroll horizontally (wheel / trackpad) instead of pushing
                // the branding out of the window.
                scrollable(
                    row![tabs, new_tab, shell_menu]
                        .spacing(theme::space::M)
                        .align_y(iced::Alignment::Center)
                )
                .id(TABS_ID)
                .direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::new().width(0).scroller_width(0),
                ))
                .width(Length::Fill),
                text("Erik van de Blaak Edition")
                    .size(theme::size::SMALL)
                    .color(theme::text_muted())
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(theme::space::M)
            .align_y(iced::Alignment::Center),
        )
        .padding([0, 14])
        .height(Length::Fixed(menu::BAR_HEIGHT))
        .align_y(iced::Alignment::Center)
        .width(Length::Fill)
        .style(|_| bar_style())
        .into()
    }

    pub(super) fn view_panes<'a>(&'a self, tab: &'a crate::workspace::Tab) -> Element<'a, Message> {
        let multiple = tab.panes.len() > 1;
        let focus = tab.focus;
        PaneGrid::new(&tab.panes, move |pane, id, _maximized| {
            let body: Element<'_, Message> = match self.terminals.get(id) {
                Some(t) => self.view_terminal(t),
                None => space::vertical().into(),
            };
            let focused = pane == focus;
            let body = container(body).padding(if multiple { 1 } else { 0 });
            pane_grid::Content::new(body).style(move |_| container::Style {
                border: Border {
                    color: if multiple && focused {
                        theme::cyan()
                    } else {
                        theme::border()
                    },
                    width: if multiple { 1.0 } else { 0.0 },
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
        })
        .spacing(theme::space::XXS)
        .on_click(Message::FocusPane)
        .on_resize(6, Message::PaneResized)
        .into()
    }

    pub(super) fn view_terminal<'a>(&'a self, t: &'a TerminalPane) -> Element<'a, Message> {
        let canvas = Canvas::new(TerminalCanvas {
            // Hollow cursor while the docked Prompt Studio has the keyboard.
            focused: self.workspace.focused() == Some(t.id)
                && !(self.prompt_docked && self.prompt_focused)
                && !(self.panel_open(SidePanel::Explorer) && self.explorer_focused),
            id: t.id,
            snapshot: &t.snapshot,
            cache: &t.cache,
            metrics: self.metrics,
            modes: t.modes,
            grid: t.grid,
        })
        .width(Length::Fill)
        .height(Length::Fill);

        let notice = match &t.status {
            Status::Running => None,
            Status::Exited(code) => Some((
                format!(
                    "Process exited{} — Enter: restart{}",
                    code.map(|c| format!(" with code {c}")).unwrap_or_default(),
                    self.keymap
                        .hint(CommandId::ClosePane)
                        .map(|k| format!(" · {k}: close"))
                        .unwrap_or_default()
                ),
                theme::amber(),
            )),
            Status::Failed(e) => Some((
                format!("Could not start {}: {e}", t.shell.name),
                theme::coral(),
            )),
        };
        match notice {
            None => canvas.into(),
            Some((msg, color)) => column![
                canvas,
                container(text(msg).size(theme::size::SMALL).color(color))
                    .padding([6, 12])
                    .width(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(theme::surface().into()),
                        ..Default::default()
                    }),
            ]
            .into(),
        }
    }

    pub(super) fn view_status_bar(&self) -> Element<'_, Message> {
        let focused = self.focused();
        let (status, color) = match focused.map(|t| &t.status) {
            Some(Status::Running) => ("● Running".to_owned(), theme::mint()),
            Some(Status::Exited(Some(code))) => (format!("● Exited ({code})"), theme::amber()),
            Some(Status::Exited(None)) => ("● Exited".to_owned(), theme::amber()),
            Some(Status::Failed(_)) => ("● Failed".to_owned(), theme::coral()),
            None => ("—".to_owned(), theme::text_muted()),
        };
        let shell = focused.map_or_else(|| "—".to_owned(), |t| t.shell.name.clone());
        let grid = focused.map_or_else(String::new, |t| format!("{}×{}", t.grid.0, t.grid.1));
        let cwd = focused
            .and_then(TerminalPane::directory)
            .map_or_else(String::new, short_path);
        let scrolled = focused
            .filter(|t| t.snapshot.display_offset > 0)
            .map_or_else(String::new, |t| {
                format!("↑ {} / {}", t.snapshot.display_offset, t.snapshot.history)
            });
        let git_summary: Element<'_, Message> = match self.git.summary() {
            Some((summary, color)) => mouse_area(
                text(summary)
                    .size(theme::size::SMALL)
                    .color(color)
                    .wrapping(text::Wrapping::None),
            )
            .on_press(Message::Command(CommandId::ToggleGitPanel))
            .into(),
            None => space::horizontal().width(0).into(),
        };
        let layout = self.workspace.active_tab().map_or_else(String::new, |tab| {
            format!(
                "Tab {}/{} · {} pane{}{}",
                self.workspace.active + 1,
                self.workspace.tabs.len(),
                tab.panes.len(),
                if tab.panes.len() == 1 { "" } else { "s" },
                if tab.panes.maximized().is_some() {
                    match self.keymap.hint(CommandId::TogglePaneZoom) {
                        Some(k) => format!(" · zoomed ({k})"),
                        None => " · zoomed".to_owned(),
                    }
                } else {
                    String::new()
                }
            )
        });

        container(
            row![
                text(status).size(theme::size::SMALL).color(color),
                text(shell).size(theme::size::SMALL).color(theme::text()),
                text(cwd)
                    .size(theme::size::SMALL)
                    .color(theme::text())
                    .wrapping(text::Wrapping::None),
                git_summary,
                text(grid)
                    .size(theme::size::SMALL)
                    .color(theme::text_muted()),
                text(layout)
                    .size(theme::size::SMALL)
                    .color(theme::text_muted()),
                text(scrolled)
                    .size(theme::size::SMALL)
                    .color(theme::violet()),
                space::horizontal(),
                text("Created by Erik van de Blaak")
                    .size(theme::size::SMALL)
                    .color(theme::text_muted()),
            ]
            .spacing(theme::space::XXL)
            .align_y(iced::Alignment::Center),
        )
        .padding([5, 14])
        .width(Length::Fill)
        .style(|_| bar_style())
        .into()
    }
}
