//! Application state, update loop and layout: tabs of split terminal panes.

use std::collections::HashMap;
use std::fmt;

use forgeterm_terminal::{
    Palette, SessionEvent, ShellSpec, detect_shells, encode_key, paste_needs_confirmation,
};
use iced::keyboard::{self, key::Named};
use iced::widget::canvas::Canvas;
use iced::widget::pane_grid::{self, Axis, Direction, PaneGrid};
use iced::widget::{button, column, container, pick_list, row, space, svg, text};
use iced::{Border, Color, Element, Event, Length, Subscription, Task, clipboard, event};

use crate::terminal_pane::{Outcome, Status, TermId, TermMsg, TerminalPane};
use crate::terminal_view::{Metrics, TerminalCanvas};
use crate::workspace::Workspace;
use crate::{keys, theme};

const LOGO: &[u8] = include_bytes!("../../../assets/branding/forgeterm-logo.svg");
const DEFAULT_FONT_SIZE: f32 = 14.0;
const MAX_TAB_TITLE: usize = 28;

#[derive(Debug, Clone)]
pub enum Message {
    /// Event from a terminal's session; stale generations are ignored by the pane.
    Session {
        id: TermId,
        generation: u64,
        event: SessionEvent,
    },
    Term(TermId, TermMsg),
    KeyPressed {
        key: keyboard::Key,
        text: Option<String>,
        modifiers: keyboard::Modifiers,
    },
    /// New tab; `None` = default shell, `Some(i)` = `detect_shells()[i]`.
    NewTab(Option<usize>),
    ShellPicked(ShellChoice),
    SelectTab(usize),
    CloseTab(usize),
    Split(Axis),
    ClosePane,
    FocusPane(pane_grid::Pane),
    PaneResized(pane_grid::ResizeEvent),
    Paste,
    ClipboardText(Option<String>),
    ConfirmPaste,
    CancelPaste,
}

/// Entry of the "new tab with shell…" dropdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellChoice {
    index: usize,
    name: String,
}

impl fmt::Display for ShellChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

pub struct App {
    terminals: HashMap<TermId, TerminalPane>,
    workspace: Workspace,
    next_id: TermId,
    shells: Vec<ShellSpec>,
    palette: Palette,
    metrics: Metrics,
    /// Multi-line paste waiting for explicit confirmation: (target terminal, text).
    pending_paste: Option<(TermId, String)>,
    /// Fatal problem shown instead of a terminal (e.g. no shell installed).
    error: Option<String>,
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let mut app = Self {
            terminals: HashMap::new(),
            workspace: Workspace::default(),
            next_id: 1,
            shells: detect_shells(),
            palette: Palette::default(),
            metrics: Metrics::new(DEFAULT_FONT_SIZE),
            pending_paste: None,
            error: None,
        };
        let task = app.new_tab(None);
        (app, task)
    }

    // ---- terminal lifecycle -------------------------------------------------------

    fn spawn_terminal(&mut self, shell: ShellSpec) -> (TermId, Task<Message>) {
        let id = self.next_id;
        self.next_id += 1;
        let cwd = std::env::current_dir().ok();
        let (pane, task) = TerminalPane::new(id, shell, cwd, self.metrics, &self.palette);
        self.terminals.insert(id, pane);
        (id, task)
    }

    fn shell_at(&self, index: Option<usize>) -> Option<ShellSpec> {
        match index {
            Some(i) => self.shells.get(i).cloned(),
            None => self.shells.first().cloned(),
        }
    }

    fn new_tab(&mut self, shell_index: Option<usize>) -> Task<Message> {
        let Some(shell) = self.shell_at(shell_index) else {
            self.error = Some("No supported shell was found on this system.".into());
            return Task::none();
        };
        let (id, task) = self.spawn_terminal(shell);
        self.workspace.push_tab(id);
        task
    }

    fn split(&mut self, axis: Axis) -> Task<Message> {
        // A split starts the same shell as the pane it splits.
        let shell = self
            .workspace
            .focused()
            .and_then(|id| self.terminals.get(&id))
            .map(|t| t.shell.clone())
            .or_else(|| self.shell_at(None));
        let Some(shell) = shell else {
            return Task::none();
        };
        let (id, task) = self.spawn_terminal(shell);
        let split = self
            .workspace
            .active_tab_mut()
            .and_then(|tab| tab.split(axis, id));
        if split.is_some() {
            self.refresh_visible();
            task
        } else {
            self.terminals.remove(&id);
            Task::none()
        }
    }

    /// Closes a terminal's pane (and its tab if it was the last pane).
    /// Exits the app when nothing is left.
    fn close_terminal(&mut self, id: TermId) -> Task<Message> {
        self.workspace.remove_terminal(id);
        self.terminals.remove(&id);
        if self.pending_paste.as_ref().is_some_and(|(t, _)| *t == id) {
            self.pending_paste = None;
        }
        self.after_layout_change()
    }

    fn close_tab(&mut self, index: usize) -> Task<Message> {
        for id in self.workspace.remove_tab(index) {
            self.terminals.remove(&id);
        }
        self.after_layout_change()
    }

    fn after_layout_change(&mut self) -> Task<Message> {
        if self.workspace.tabs.is_empty() {
            return iced::exit();
        }
        self.refresh_visible();
        Task::none()
    }

    /// Brings terminals of the active tab up to date and forces a redraw, e.g. after a
    /// tab switch or focus change (focus changes the cursor style).
    fn refresh_visible(&mut self) {
        let Some(tab) = self.workspace.active_tab() else {
            return;
        };
        for id in tab.terminals() {
            if let Some(t) = self.terminals.get_mut(&id) {
                t.refresh_if_dirty(&self.palette);
                t.cache.clear();
            }
        }
    }

    fn focused(&self) -> Option<&TerminalPane> {
        self.workspace
            .focused()
            .and_then(|id| self.terminals.get(&id))
    }

    fn focus_terminal(&mut self, id: TermId) {
        if self.workspace.focused() == Some(id) {
            return;
        }
        if let Some((tab_index, pane)) = self.workspace.locate(id) {
            self.workspace.select(tab_index);
            if let Some(tab) = self.workspace.active_tab_mut() {
                tab.focus = pane;
            }
            self.refresh_visible();
        }
    }

    // ---- iced entry points --------------------------------------------------------

    pub fn title(&self) -> String {
        match self.focused() {
            Some(t) => format!("{} — ForgeTerm", t.display_title()),
            None => "ForgeTerm".into(),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        event::listen_with(|event, _status, _window| match event {
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                text,
                modifiers,
                ..
            }) => Some(Message::KeyPressed {
                key,
                text: text.map(|t| t.to_string()),
                modifiers,
            }),
            _ => None,
        })
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Session {
                id,
                generation,
                event,
            } => {
                let visible = self.workspace.is_visible(id);
                let Some(terminal) = self.terminals.get_mut(&id) else {
                    return Task::none();
                };
                match terminal.on_session_event(generation, event, visible, &self.palette) {
                    Outcome::None => {}
                    Outcome::Task(task) => return task,
                    Outcome::Close => return self.close_terminal(id),
                }
            }
            Message::Term(id, msg) => return self.on_term_msg(id, msg),
            Message::KeyPressed {
                key,
                text,
                modifiers,
            } => return self.on_key(key, text, modifiers),
            Message::NewTab(index) => return self.new_tab(index),
            Message::ShellPicked(choice) => return self.new_tab(Some(choice.index)),
            Message::SelectTab(index) => {
                if self.workspace.select(index) {
                    self.refresh_visible();
                }
            }
            Message::CloseTab(index) => return self.close_tab(index),
            Message::Split(axis) => return self.split(axis),
            Message::ClosePane => {
                if let Some(id) = self.workspace.focused() {
                    return self.close_terminal(id);
                }
            }
            Message::FocusPane(pane) => {
                if let Some(tab) = self.workspace.active_tab_mut()
                    && tab.focus != pane
                {
                    tab.focus = pane;
                    self.refresh_visible();
                }
            }
            Message::PaneResized(pane_grid::ResizeEvent { split, ratio }) => {
                if let Some(tab) = self.workspace.active_tab_mut() {
                    tab.panes.resize(split, ratio);
                }
            }
            Message::Paste => return clipboard::read().map(Message::ClipboardText),
            Message::ClipboardText(Some(text)) => {
                let Some(t) = self.focused() else {
                    return Task::none();
                };
                if paste_needs_confirmation(&text, t.modes.bracketed_paste) {
                    self.pending_paste = Some((t.id, text));
                } else if let Some(s) = t.session() {
                    s.paste(&text);
                }
            }
            Message::ClipboardText(None) => {}
            Message::ConfirmPaste => {
                if let Some((id, text)) = self.pending_paste.take()
                    && let Some(s) = self.terminals.get(&id).and_then(TerminalPane::session)
                {
                    s.paste(&text);
                }
            }
            Message::CancelPaste => self.pending_paste = None,
        }
        Task::none()
    }

    fn on_term_msg(&mut self, id: TermId, msg: TermMsg) -> Task<Message> {
        if msg.focuses() {
            self.focus_terminal(id);
        }
        let (metrics, palette) = (self.metrics, &self.palette);
        let Some(t) = self.terminals.get_mut(&id) else {
            return Task::none();
        };
        match msg {
            TermMsg::GridResized(cols, rows) => t.resize(cols, rows, metrics, palette),
            TermMsg::Scroll(lines) => {
                if let Some(s) = t.session() {
                    s.scroll(lines);
                }
                t.refresh(palette);
            }
            TermMsg::SelectStart(c, r, right) => {
                if let Some(s) = t.session() {
                    s.start_selection(c, r, right);
                }
                t.refresh(palette);
            }
            TermMsg::SelectUpdate(c, r, right) => {
                if let Some(s) = t.session() {
                    s.update_selection(c, r, right);
                }
                t.refresh(palette);
            }
            TermMsg::SelectEnd => {}
            TermMsg::Input { bytes, .. } => {
                if let Some(s) = t.session() {
                    s.write(bytes);
                }
            }
            TermMsg::Paste => return Task::done(Message::Paste),
        }
        Task::none()
    }

    fn set_font_size(&mut self, size: f32) {
        self.metrics = Metrics::new(size.clamp(8.0, 32.0));
        for t in self.terminals.values() {
            t.cache.clear();
        }
    }

    fn on_key(
        &mut self,
        key: keyboard::Key,
        text: Option<String>,
        mods: keyboard::Modifiers,
    ) -> Task<Message> {
        // While a paste confirmation is open, the keyboard answers it and nothing else.
        if self.pending_paste.is_some() {
            return match key {
                keyboard::Key::Named(Named::Enter) => self.update(Message::ConfirmPaste),
                keyboard::Key::Named(Named::Escape) => self.update(Message::CancelPaste),
                _ => Task::none(),
            };
        }

        if let Some(task) = self.app_shortcut(&key, mods) {
            return task;
        }

        let Some(focused) = self.workspace.focused() else {
            return Task::none();
        };
        let Some(t) = self.terminals.get_mut(&focused) else {
            return Task::none();
        };

        // A pane whose shell ended: Enter starts it again.
        if !t.is_running() {
            if matches!(key, keyboard::Key::Named(Named::Enter)) {
                return t.start(self.metrics, &self.palette);
            }
            return Task::none();
        }

        if let Some((tkey, tmods)) = keys::translate(&key, text.as_deref(), mods)
            && let Some(bytes) = encode_key(&tkey, tmods, t.modes.app_cursor)
            && let Some(session) = t.session()
        {
            session.input(bytes);
        }
        Task::none()
    }

    /// Application-level shortcuts. Returns `None` when the key belongs to the terminal.
    fn app_shortcut(
        &mut self,
        key: &keyboard::Key,
        mods: keyboard::Modifiers,
    ) -> Option<Task<Message>> {
        let (ctrl, shift, alt) = (mods.control(), mods.shift(), mods.alt());
        let ch = match key {
            keyboard::Key::Character(c) => c.to_lowercase(),
            _ => String::new(),
        };
        let digit = ch.parse::<usize>().ok().filter(|d| (1..=9).contains(d));
        let named = |n: Named| matches!(key, keyboard::Key::Named(k) if *k == n);

        // Tabs and panes.
        if ctrl && shift && !alt {
            match ch.as_str() {
                "t" => return Some(self.update(Message::NewTab(None))),
                "w" => return Some(self.update(Message::ClosePane)),
                "c" => return Some(self.copy_selection()),
                "v" => return Some(Task::done(Message::Paste)),
                _ => {}
            }
            if let Some(d) = digit {
                return Some(self.update(Message::NewTab(Some(d - 1))));
            }
        }
        if ctrl && !alt && named(Named::Tab) {
            self.workspace.cycle(!shift);
            self.refresh_visible();
            return Some(Task::none());
        }
        // Alt+N selects tab N (GNOME Terminal convention). Not Ctrl+Alt+N: on Windows
        // that is AltGr, which types characters on many layouts (US-Intl: Ctrl+Alt+1 = ¡).
        if alt
            && !ctrl
            && !shift
            && let Some(d) = digit
        {
            return Some(self.update(Message::SelectTab(d - 1)));
        }
        if alt && shift && !ctrl {
            match ch.as_str() {
                "=" | "+" => return Some(self.update(Message::Split(Axis::Vertical))),
                "-" | "_" => return Some(self.update(Message::Split(Axis::Horizontal))),
                _ => {}
            }
        }
        if alt && !shift && !ctrl {
            let direction = match key {
                keyboard::Key::Named(Named::ArrowLeft) => Some(Direction::Left),
                keyboard::Key::Named(Named::ArrowRight) => Some(Direction::Right),
                keyboard::Key::Named(Named::ArrowUp) => Some(Direction::Up),
                keyboard::Key::Named(Named::ArrowDown) => Some(Direction::Down),
                _ => None,
            };
            if let Some(direction) = direction
                && let Some(tab) = self.workspace.active_tab_mut()
                && tab.panes.len() > 1
            {
                if tab.focus_adjacent(direction) {
                    self.refresh_visible();
                }
                return Some(Task::none());
            }
        }

        // Clipboard, zoom, scrollback.
        if shift && !ctrl && named(Named::Insert) {
            return Some(Task::done(Message::Paste));
        }
        if ctrl && !alt && !shift {
            match ch.as_str() {
                "=" | "+" => {
                    self.set_font_size(self.metrics.font_size + 1.0);
                    return Some(Task::none());
                }
                "-" => {
                    self.set_font_size(self.metrics.font_size - 1.0);
                    return Some(Task::none());
                }
                "0" => {
                    self.set_font_size(DEFAULT_FONT_SIZE);
                    return Some(Task::none());
                }
                // Ctrl+C with an active selection copies instead of sending SIGINT.
                "c" if self
                    .focused()
                    .and_then(TerminalPane::session)
                    .is_some_and(|s| s.selection_text().is_some()) =>
                {
                    return Some(self.copy_selection());
                }
                _ => {}
            }
        }
        if shift && !ctrl && !alt && (named(Named::PageUp) || named(Named::PageDown)) {
            let id = self.workspace.focused()?;
            let rows = self.terminals.get(&id)?.grid.1;
            let page = i32::from(rows.saturating_sub(1).max(1));
            let lines = if named(Named::PageUp) { page } else { -page };
            return Some(self.on_term_msg(id, TermMsg::Scroll(lines)));
        }
        None
    }

    fn copy_selection(&mut self) -> Task<Message> {
        let Some(id) = self.workspace.focused() else {
            return Task::none();
        };
        let Some(t) = self.terminals.get_mut(&id) else {
            return Task::none();
        };
        let Some(selected) = t.session().and_then(|s| s.selection_text()) else {
            return Task::none();
        };
        if let Some(s) = t.session() {
            s.clear_selection();
        }
        t.refresh(&self.palette);
        let selected = if cfg!(windows) {
            selected.replace('\n', "\r\n")
        } else {
            selected
        };
        clipboard::write(selected)
    }

    // ---- view ---------------------------------------------------------------------

    pub fn view(&self) -> Element<'_, Message> {
        let mut layout = column![self.view_top_bar()];
        if let Some((_, pending)) = &self.pending_paste {
            layout = layout.push(paste_banner(pending));
        }
        let body: Element<'_, Message> = match (&self.error, self.workspace.active_tab()) {
            (Some(err), _) => container(text(err.as_str()).color(theme::CORAL))
                .center(Length::Fill)
                .into(),
            (None, Some(tab)) => self.view_panes(tab),
            (None, None) => space::vertical().into(),
        };
        layout.push(body).push(self.view_status_bar()).into()
    }

    fn view_top_bar(&self) -> Element<'_, Message> {
        let mut tabs = row![].spacing(4).align_y(iced::Alignment::Center);
        for (index, tab) in self.workspace.tabs.iter().enumerate() {
            let active = index == self.workspace.active;
            let focused = tab.focused().and_then(|id| self.terminals.get(&id));
            let title = focused.map_or_else(|| "—".to_owned(), TerminalPane::display_title);
            let dot = match focused.map(|t| &t.status) {
                Some(Status::Running) | None => theme::MINT,
                Some(Status::Exited(_)) => theme::AMBER,
                Some(Status::Failed(_)) => theme::CORAL,
            };
            let panes = tab.panes.len();
            let label = if panes > 1 {
                format!("{}  ·{panes}", truncate(&title, MAX_TAB_TITLE))
            } else {
                truncate(&title, MAX_TAB_TITLE)
            };
            let select = button(
                row![
                    text("●").size(9).color(dot),
                    text(label).size(12).color(if active {
                        theme::TEXT
                    } else {
                        theme::TEXT_MUTED
                    }),
                ]
                .spacing(7)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::SelectTab(index))
            .padding([5, 4])
            .style(|_, _| button::Style::default());
            let close = button(text("×").size(13).color(theme::TEXT_MUTED))
                .on_press(Message::CloseTab(index))
                .padding([1, 6])
                .style(|_, status| ghost_button(status, theme::CORAL));
            tabs = tabs.push(
                container(
                    row![select, close]
                        .spacing(2)
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
                index,
                name: s.name.clone(),
            })
            .collect();
        let new_tab = button(text("+").size(15).color(theme::TEXT))
            .on_press(Message::NewTab(None))
            .padding([2, 9])
            .style(|_, status| ghost_button(status, theme::CYAN));
        let shell_menu = pick_list(choices, None::<ShellChoice>, Message::ShellPicked)
            .placeholder("New tab with…")
            .width(Length::Fixed(150.0))
            .text_size(12)
            .padding([3, 8]);

        container(
            row![
                svg(svg::Handle::from_memory(LOGO)).width(22).height(22),
                text("ForgeTerm").size(15).color(theme::TEXT),
                space::horizontal().width(10),
                tabs,
                new_tab,
                shell_menu,
                space::horizontal(),
                text("Erik van de Blaak Edition")
                    .size(12)
                    .color(theme::TEXT_MUTED),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        )
        .padding([6, 14])
        .width(Length::Fill)
        .style(|_| bar_style())
        .into()
    }

    fn view_panes<'a>(&'a self, tab: &'a crate::workspace::Tab) -> Element<'a, Message> {
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
                        theme::CYAN
                    } else {
                        theme::BORDER
                    },
                    width: if multiple { 1.0 } else { 0.0 },
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
        })
        .spacing(2)
        .on_click(Message::FocusPane)
        .on_resize(6, Message::PaneResized)
        .into()
    }

    fn view_terminal<'a>(&'a self, t: &'a TerminalPane) -> Element<'a, Message> {
        let canvas = Canvas::new(TerminalCanvas {
            focused: self.workspace.focused() == Some(t.id),
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
                    "Process exited{} — Enter: restart · Ctrl+Shift+W: close",
                    code.map(|c| format!(" with code {c}")).unwrap_or_default()
                ),
                theme::AMBER,
            )),
            Status::Failed(e) => Some((
                format!("Could not start {}: {e}", t.shell.name),
                theme::CORAL,
            )),
        };
        match notice {
            None => canvas.into(),
            Some((msg, color)) => column![
                canvas,
                container(text(msg).size(12).color(color))
                    .padding([6, 12])
                    .width(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(theme::SURFACE.into()),
                        ..Default::default()
                    }),
            ]
            .into(),
        }
    }

    fn view_status_bar(&self) -> Element<'_, Message> {
        let focused = self.focused();
        let (status, color) = match focused.map(|t| &t.status) {
            Some(Status::Running) => ("● Running".to_owned(), theme::MINT),
            Some(Status::Exited(Some(code))) => (format!("● Exited ({code})"), theme::AMBER),
            Some(Status::Exited(None)) => ("● Exited".to_owned(), theme::AMBER),
            Some(Status::Failed(_)) => ("● Failed".to_owned(), theme::CORAL),
            None => ("—".to_owned(), theme::TEXT_MUTED),
        };
        let shell = focused.map_or_else(|| "—".to_owned(), |t| t.shell.name.clone());
        let grid = focused.map_or_else(String::new, |t| format!("{}×{}", t.grid.0, t.grid.1));
        let scrolled = focused
            .filter(|t| t.snapshot.display_offset > 0)
            .map_or_else(String::new, |t| {
                format!("↑ {} / {}", t.snapshot.display_offset, t.snapshot.history)
            });
        let layout = self.workspace.active_tab().map_or_else(String::new, |tab| {
            format!(
                "Tab {}/{} · {} pane{}",
                self.workspace.active + 1,
                self.workspace.tabs.len(),
                tab.panes.len(),
                if tab.panes.len() == 1 { "" } else { "s" }
            )
        });

        container(
            row![
                text(status).size(12).color(color),
                text(shell).size(12).color(theme::TEXT),
                text(grid).size(12).color(theme::TEXT_MUTED),
                text(layout).size(12).color(theme::TEXT_MUTED),
                text(scrolled).size(12).color(theme::VIOLET),
                space::horizontal(),
                text("Created by Erik van de Blaak")
                    .size(12)
                    .color(theme::TEXT_MUTED),
            ]
            .spacing(16)
            .align_y(iced::Alignment::Center),
        )
        .padding([5, 14])
        .width(Length::Fill)
        .style(|_| bar_style())
        .into()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

fn tab_style(active: bool) -> container::Style {
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

fn ghost_button(status: button::Status, hover: Color) -> button::Style {
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

fn paste_banner(text_to_paste: &str) -> Element<'_, Message> {
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

fn bar_style() -> container::Style {
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
