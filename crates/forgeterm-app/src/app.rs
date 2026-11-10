//! Application state, update loop and layout.

use forgeterm_terminal::{
    GridSize, Modes, Palette, Session, SessionConfig, SessionEvent, Snapshot, encode_key,
    paste_needs_confirmation,
};
use iced::futures::channel::mpsc;
use iced::keyboard::{self, key::Named};
use iced::widget::canvas::{Cache, Canvas};
use iced::widget::{button, column, container, row, space, svg, text};
use iced::{Border, Element, Event, Length, Subscription, Task, clipboard, event};

use crate::terminal_view::{Metrics, TerminalCanvas};
use crate::{keys, theme};

const LOGO: &[u8] = include_bytes!("../../../assets/branding/forgeterm-logo.svg");
const DEFAULT_FONT_SIZE: f32 = 14.0;

#[derive(Debug, Clone)]
pub enum Message {
    /// Event from session number `.0` (stale generations are ignored).
    Session(u64, SessionEvent),
    KeyPressed {
        key: keyboard::Key,
        text: Option<String>,
        modifiers: keyboard::Modifiers,
    },
    GridResized(u16, u16),
    Scroll(i32),
    SelectStart(usize, usize, bool),
    SelectUpdate(usize, usize, bool),
    SelectEnd,
    PtyInput(Vec<u8>),
    Paste,
    ClipboardText(Option<String>),
    ConfirmPaste,
    CancelPaste,
}

enum Status {
    Running,
    Exited(Option<i32>),
    Failed(String),
}

pub struct App {
    session: Option<Session>,
    generation: u64,
    status: Status,
    palette: Palette,
    snapshot: Snapshot,
    modes: Modes,
    cache: Cache,
    metrics: Metrics,
    grid: (u16, u16),
    title: Option<String>,
    /// Multi-line paste waiting for the user's explicit confirmation.
    pending_paste: Option<String>,
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let palette = Palette::default();
        let metrics = Metrics::new(DEFAULT_FONT_SIZE);
        let mut app = Self {
            session: None,
            generation: 0,
            status: Status::Running,
            snapshot: blank_snapshot(&palette, 80, 24),
            palette,
            modes: Modes::default(),
            cache: Cache::new(),
            metrics,
            grid: (80, 24),
            title: None,
            pending_paste: None,
        };
        let task = app.start_session();
        (app, task)
    }

    fn grid_size(&self) -> GridSize {
        GridSize::new(
            self.grid.0,
            self.grid.1,
            self.metrics.cell_width.round() as u16,
            self.metrics.cell_height.round() as u16,
        )
    }

    /// Spawns the default shell and returns the task that streams its events.
    fn start_session(&mut self) -> Task<Message> {
        self.generation += 1;
        let generation = self.generation;
        let (tx, rx) = mpsc::unbounded();
        let config = SessionConfig {
            working_directory: std::env::current_dir().ok(),
            size: self.grid_size(),
            palette: self.palette.clone(),
            ..Default::default()
        };
        match Session::spawn(config, move |e| {
            let _ = tx.unbounded_send(e);
        }) {
            Ok(session) => {
                self.session = Some(session);
                self.status = Status::Running;
                self.title = None;
                self.refresh();
                Task::run(rx, move |e| Message::Session(generation, e))
            }
            Err(err) => {
                tracing::error!("failed to start session: {err:#}");
                self.status = Status::Failed(format!("{err:#}"));
                Task::none()
            }
        }
    }

    fn refresh(&mut self) {
        if let Some(session) = &self.session {
            self.snapshot = session.snapshot(&self.palette);
            self.modes = session.modes();
            self.cache.clear();
        }
    }

    pub fn title(&self) -> String {
        match &self.title {
            Some(t) if !t.is_empty() => format!("{t} — ForgeTerm"),
            _ => "ForgeTerm".into(),
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
            Message::Session(generation, _) if generation != self.generation => {}
            Message::Session(_, event) => match event {
                SessionEvent::Wakeup => self.refresh(),
                SessionEvent::Title(t) => self.title = Some(t),
                SessionEvent::ResetTitle => self.title = None,
                SessionEvent::Bell => {}
                SessionEvent::ClipboardStore(t) => return clipboard::write(t),
                SessionEvent::Exited(code) => {
                    self.refresh();
                    self.status = Status::Exited(code);
                    self.session = None;
                }
            },
            Message::KeyPressed {
                key,
                text,
                modifiers,
            } => return self.on_key(key, text, modifiers),
            Message::GridResized(cols, rows) => {
                self.grid = (cols, rows);
                let size = self.grid_size();
                if let Some(session) = &mut self.session {
                    session.resize(size);
                }
                self.refresh();
            }
            Message::Scroll(lines) => {
                if let Some(s) = &self.session {
                    s.scroll(lines);
                }
                self.refresh();
            }
            Message::SelectStart(c, r, right) => {
                if let Some(s) = &self.session {
                    s.start_selection(c, r, right);
                }
                self.refresh();
            }
            Message::SelectUpdate(c, r, right) => {
                if let Some(s) = &self.session {
                    s.update_selection(c, r, right);
                }
                self.refresh();
            }
            Message::SelectEnd => {}
            Message::PtyInput(bytes) => {
                if let Some(s) = &self.session {
                    s.write(bytes);
                }
            }
            Message::Paste => return clipboard::read().map(Message::ClipboardText),
            Message::ClipboardText(Some(t)) => {
                if paste_needs_confirmation(&t, self.modes.bracketed_paste) {
                    self.pending_paste = Some(t);
                } else if let Some(s) = &self.session {
                    s.paste(&t);
                }
            }
            Message::ClipboardText(None) => {}
            Message::ConfirmPaste => {
                if let (Some(t), Some(s)) = (self.pending_paste.take(), &self.session) {
                    s.paste(&t);
                }
            }
            Message::CancelPaste => self.pending_paste = None,
        }
        Task::none()
    }

    fn set_font_size(&mut self, size: f32) {
        self.metrics = Metrics::new(size.clamp(8.0, 32.0));
        self.cache.clear();
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

        let ctrl_shift = mods.control() && mods.shift();
        let char_key = |k: &keyboard::Key| match k {
            keyboard::Key::Character(c) => c.to_lowercase(),
            _ => String::new(),
        };

        // Application shortcuts.
        if ctrl_shift && char_key(&key) == "c" {
            return self.copy_selection();
        }
        if (ctrl_shift && char_key(&key) == "v")
            || (mods.shift() && matches!(key, keyboard::Key::Named(Named::Insert)))
        {
            return Task::done(Message::Paste);
        }
        if mods.control() && !mods.alt() {
            match char_key(&key).as_str() {
                "=" | "+" => {
                    self.set_font_size(self.metrics.font_size + 1.0);
                    return Task::none();
                }
                "-" => {
                    self.set_font_size(self.metrics.font_size - 1.0);
                    return Task::none();
                }
                "0" => {
                    self.set_font_size(DEFAULT_FONT_SIZE);
                    return Task::none();
                }
                // Ctrl+C with an active selection copies instead of sending SIGINT.
                "c" if !mods.shift() => {
                    if let Some(s) = &self.session
                        && s.selection_text().is_some()
                    {
                        return self.copy_selection();
                    }
                }
                _ => {}
            }
        }
        if mods.shift() && !mods.control() {
            let page = i32::from(self.grid.1.saturating_sub(1).max(1));
            match key {
                keyboard::Key::Named(Named::PageUp) => return self.update(Message::Scroll(page)),
                keyboard::Key::Named(Named::PageDown) => {
                    return self.update(Message::Scroll(-page));
                }
                _ => {}
            }
        }

        // Exited shell: Enter starts a fresh one.
        if self.session.is_none() {
            if matches!(key, keyboard::Key::Named(Named::Enter)) {
                return self.start_session();
            }
            return Task::none();
        }

        if let Some((tkey, tmods)) = keys::translate(&key, text.as_deref(), mods)
            && let Some(bytes) = encode_key(&tkey, tmods, self.modes.app_cursor)
            && let Some(session) = &self.session
        {
            session.input(bytes);
        }
        Task::none()
    }

    fn copy_selection(&mut self) -> Task<Message> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        let Some(selected) = session.selection_text() else {
            return Task::none();
        };
        session.clear_selection();
        self.refresh();
        let selected = if cfg!(windows) {
            selected.replace('\n', "\r\n")
        } else {
            selected
        };
        clipboard::write(selected)
    }

    pub fn view(&self) -> Element<'_, Message> {
        let top_bar = container(
            row![
                svg(svg::Handle::from_memory(LOGO)).width(22).height(22),
                text("ForgeTerm").size(15).color(theme::TEXT),
                text("Erik van de Blaak Edition")
                    .size(12)
                    .color(theme::TEXT_MUTED),
                space::horizontal(),
                text(self.title.clone().unwrap_or_default())
                    .size(12)
                    .color(theme::TEXT_MUTED),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center),
        )
        .padding([8, 14])
        .width(Length::Fill)
        .style(|_| bar_style());

        let terminal = Canvas::new(TerminalCanvas {
            snapshot: &self.snapshot,
            cache: &self.cache,
            metrics: self.metrics,
            modes: self.modes,
            grid: self.grid,
        })
        .width(Length::Fill)
        .height(Length::Fill);

        let shell = self
            .session
            .as_ref()
            .map(|s| s.shell().name.clone())
            .unwrap_or_else(|| "—".into());
        let (status, color) = match &self.status {
            Status::Running => ("● Running".to_owned(), theme::MINT),
            Status::Exited(Some(code)) => (
                format!("● Exited ({code}) — press Enter to restart"),
                theme::AMBER,
            ),
            Status::Exited(None) => ("● Exited — press Enter to restart".to_owned(), theme::AMBER),
            Status::Failed(e) => (format!("● Failed to start shell: {e}"), theme::CORAL),
        };
        let scrolled = if self.snapshot.display_offset > 0 {
            format!(
                "↑ {} / {}",
                self.snapshot.display_offset, self.snapshot.history
            )
        } else {
            String::new()
        };

        let status_bar = container(
            row![
                text(status).size(12).color(color),
                text(shell).size(12).color(theme::TEXT),
                text(format!("{}×{}", self.grid.0, self.grid.1))
                    .size(12)
                    .color(theme::TEXT_MUTED),
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
        .style(|_| bar_style());

        let mut layout = column![top_bar];
        if let Some(pending) = &self.pending_paste {
            layout = layout.push(paste_banner(pending));
        }
        layout.push(terminal).push(status_bar).into()
    }
}

fn paste_banner(text_to_paste: &str) -> Element<'_, Message> {
    let lines = text_to_paste.trim_end_matches(['\r', '\n']).lines().count();
    let button_style = |accent: iced::Color| {
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

fn blank_snapshot(palette: &Palette, cols: usize, rows: usize) -> Snapshot {
    let blank = forgeterm_terminal::CellView {
        c: ' ',
        zerowidth: None,
        fg: palette.foreground,
        bg: palette.background,
        flags: Default::default(),
    };
    Snapshot {
        cols,
        rows,
        cells: vec![blank; cols * rows],
        cursor: None,
        background: palette.background,
        display_offset: 0,
        history: 0,
    }
}
