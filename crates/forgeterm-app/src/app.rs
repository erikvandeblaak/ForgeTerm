//! Application state, update loop and layout: tabs of split terminal panes.

use std::collections::HashMap;
use std::fmt;

use forgeterm_terminal::{
    Palette, SessionEvent, ShellSpec, detect_shells, encode_key, paste_needs_confirmation,
};
use iced::keyboard::{self, key::Named};
use iced::widget::canvas::Canvas;
use iced::widget::pane_grid::{self, Axis, Direction, PaneGrid};
use iced::widget::{
    button, column, container, mouse_area, opaque, pick_list, row, space, stack, svg, text,
};
use iced::{
    Border, Color, Element, Event, Length, Shadow, Subscription, Task, Vector, clipboard, event,
};

use crate::commands::{CommandId, Dir, Keymap, STATIC_COMMANDS};
use crate::menu::{self, Menu, MenuState};
use crate::palette::{self as command_palette, CommandPalette, Entry};
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
        captured: bool,
    },
    ShellPicked(ShellChoice),
    /// Tab-bar clicks (not recorded as "recent commands").
    SelectTab(usize),
    CloseTab(usize),
    FocusPane(pane_grid::Pane),
    PaneResized(pane_grid::ResizeEvent),
    Paste,
    ClipboardText(Option<String>),
    ConfirmPaste,
    CancelPaste,
    /// Run a registered command (palette, menus, buttons all go through here).
    Command(CommandId),
    PaletteQuery(String),
    /// Run the selected (`None`) or the given result row.
    PaletteRun(Option<usize>),
    PaletteHover(usize),
    PaletteClose,
    /// Click on a menu title: opens it, or closes it when it is already open.
    MenuOpen(usize),
    /// Mouse over a menu title: switches menus while one is open.
    MenuHoverTitle(usize),
    MenuHoverItem(usize),
    /// Run the entry at this index of the open menu.
    MenuRun(usize),
    MenuClose,
    AboutClose,
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
    keymap: Keymap,
    command_palette: Option<CommandPalette>,
    /// Most recently run commands, newest first (palette ordering).
    recent_commands: Vec<CommandId>,
    menus: Vec<Menu>,
    menu: Option<MenuState>,
    about_open: bool,
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
            keymap: Keymap::default(),
            command_palette: None,
            recent_commands: Vec::new(),
            menus: Vec::new(),
            menu: None,
            about_open: false,
        };
        app.menus = menu::menus(app.shells.len());
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
        self.restore_zoom();
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
        if let Some((tab_index, _)) = self.workspace.locate(id) {
            self.workspace.tabs[tab_index].panes.restore();
        }
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

    fn restore_zoom(&mut self) {
        if let Some(tab) = self.workspace.active_tab_mut() {
            tab.panes.restore();
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
        event::listen_with(|event, status, _window| match event {
            // Keys a widget already handled (e.g. Enter submitting the palette's text
            // input) must not also reach the terminal.
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                text,
                modifiers,
                ..
            }) => Some(Message::KeyPressed {
                key,
                text: text.map(|t| t.to_string()),
                modifiers,
                // A widget (e.g. the palette's text input) already handled this key.
                captured: status == event::Status::Captured,
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
                captured,
            } => return self.on_key(key, text, modifiers, captured),
            Message::ShellPicked(choice) => {
                return self.run_command(CommandId::NewTabWithShell(choice.index));
            }
            Message::SelectTab(index) => {
                self.menu = None;
                if self.workspace.select(index) {
                    self.refresh_visible();
                }
            }
            Message::CloseTab(index) => {
                self.menu = None;
                return self.close_tab(index);
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
            Message::Command(id) => return self.run_command(id),
            Message::PaletteQuery(query) => {
                if let Some(p) = &mut self.command_palette {
                    p.set_query(query);
                }
            }
            Message::PaletteHover(index) => {
                if let Some(p) = &mut self.command_palette {
                    let delta = index as i32 - p.selected_index() as i32;
                    p.move_selection(delta);
                }
            }
            Message::PaletteRun(index) => {
                let id = self.command_palette.as_ref().and_then(|p| match index {
                    Some(i) => p.command_at(i),
                    None => p.selected(),
                });
                self.command_palette = None;
                if let Some(id) = id {
                    return self.run_command(id);
                }
            }
            Message::PaletteClose => self.command_palette = None,
            Message::MenuOpen(index) => {
                self.menu = match self.menu {
                    Some(state) if state.open == index => None,
                    _ => Some(MenuState::new(index)),
                };
            }
            Message::MenuHoverTitle(index) => {
                if let Some(state) = &mut self.menu
                    && state.open != index
                {
                    *state = MenuState::new(index);
                }
            }
            Message::MenuHoverItem(index) => {
                if let Some(state) = &mut self.menu {
                    state.highlighted = Some(index);
                }
            }
            Message::MenuRun(index) => {
                let Some(state) = self.menu else {
                    return Task::none();
                };
                let rows = self.menu_rows(state.open);
                let run = MenuState {
                    highlighted: Some(index),
                    ..state
                };
                self.menu = None;
                if let Some(id) = run.highlighted_command(&rows) {
                    return self.run_command(id);
                }
            }
            Message::MenuClose => self.menu = None,
            Message::AboutClose => self.about_open = false,
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
        captured: bool,
    ) -> Task<Message> {
        // While a paste confirmation is open, the keyboard answers it and nothing else.
        if self.pending_paste.is_some() {
            return match key {
                keyboard::Key::Named(Named::Enter) => self.update(Message::ConfirmPaste),
                keyboard::Key::Named(Named::Escape) => self.update(Message::CancelPaste),
                _ => Task::none(),
            };
        }

        // Modal overlays own the keyboard while open.
        if self.about_open {
            if matches!(key, keyboard::Key::Named(Named::Escape | Named::Enter)) {
                self.about_open = false;
            }
            return Task::none();
        }
        if self.menu.is_some() {
            return self.menu_key(&key);
        }
        if self.command_palette.is_some() {
            return self.palette_key(&key, text.as_deref(), mods, captured);
        }
        // Keys handled by a widget never reach the terminal.
        if captured {
            return Task::none();
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

    fn palette_key(
        &mut self,
        key: &keyboard::Key,
        text: Option<&str>,
        mods: keyboard::Modifiers,
        captured: bool,
    ) -> Task<Message> {
        let toggle = keys::binding(key, mods)
            .and_then(|b| self.keymap.lookup(b))
            .is_some_and(|id| id == CommandId::CommandPalette);
        // Escape always closes, even though the text input also reacts to it.
        if matches!(key, keyboard::Key::Named(Named::Escape)) || toggle {
            self.command_palette = None;
            return Task::none();
        }
        // The focused text input handles typing, editing and Enter itself.
        if captured {
            return Task::none();
        }
        let Some(p) = &mut self.command_palette else {
            return Task::none();
        };
        match key {
            keyboard::Key::Named(Named::ArrowDown) => p.move_selection(1),
            keyboard::Key::Named(Named::ArrowUp) => p.move_selection(-1),
            keyboard::Key::Named(Named::PageDown) => p.move_selection(8),
            keyboard::Key::Named(Named::PageUp) => p.move_selection(-8),
            keyboard::Key::Named(Named::Tab) if !mods.control() => {
                p.move_selection(if mods.shift() { -1 } else { 1 });
            }
            // Keys that arrive before the input has focus (typing right after
            // Ctrl+Shift+P) are not lost: Enter runs, text extends the query.
            keyboard::Key::Named(Named::Enter) => {
                return self.update(Message::PaletteRun(None));
            }
            _ => {
                if !mods.control()
                    && !mods.alt()
                    && let Some(t) = text.filter(|t| !t.chars().any(char::is_control))
                {
                    p.push_query(t);
                }
            }
        }
        Task::none()
    }

    fn menu_key(&mut self, key: &keyboard::Key) -> Task<Message> {
        let Some(mut state) = self.menu else {
            return Task::none();
        };
        match key {
            keyboard::Key::Named(Named::Escape) => {
                self.menu = None;
                return Task::none();
            }
            keyboard::Key::Named(Named::ArrowLeft) => state.switch(self.menus.len(), -1),
            keyboard::Key::Named(Named::ArrowRight) => state.switch(self.menus.len(), 1),
            keyboard::Key::Named(Named::ArrowDown) => state.step(&self.menu_rows(state.open), 1),
            keyboard::Key::Named(Named::ArrowUp) => state.step(&self.menu_rows(state.open), -1),
            keyboard::Key::Named(Named::Enter | Named::Space) => {
                let command = state.highlighted_command(&self.menu_rows(state.open));
                if let Some(id) = command {
                    return self.run_command(id);
                }
            }
            // Everything else is swallowed: an open menu never types into the terminal.
            _ => {}
        }
        self.menu = Some(state);
        Task::none()
    }

    /// Application-level shortcuts. Returns `None` when the key belongs to the terminal.
    fn app_shortcut(
        &mut self,
        key: &keyboard::Key,
        mods: keyboard::Modifiers,
    ) -> Option<Task<Message>> {
        // Ctrl+C with an active selection copies instead of sending SIGINT. Conditional,
        // so it lives here rather than in the keymap.
        if mods.control()
            && !mods.shift()
            && !mods.alt()
            && matches!(key, keyboard::Key::Character(c) if c.eq_ignore_ascii_case("c"))
            && self
                .focused()
                .and_then(TerminalPane::session)
                .is_some_and(|s| s.selection_text().is_some())
        {
            return Some(self.copy_selection());
        }

        let id = self.keymap.lookup(keys::binding(key, mods)?)?;
        // Pane focus keys only make sense with several panes; otherwise Alt+Arrow
        // belongs to the program in the terminal.
        if matches!(id, CommandId::FocusPane(_)) && !self.has_splits() {
            return None;
        }
        Some(self.run_command(id))
    }

    fn has_splits(&self) -> bool {
        self.workspace
            .active_tab()
            .is_some_and(|tab| tab.panes.len() > 1)
    }

    /// The single place where commands are executed.
    pub fn run_command(&mut self, id: CommandId) -> Task<Message> {
        self.menu = None;
        if id != CommandId::CommandPalette {
            self.recent_commands.retain(|c| *c != id);
            self.recent_commands.insert(0, id);
            self.recent_commands.truncate(command_palette::RECENT_LIMIT);
        }
        match id {
            CommandId::CommandPalette => {
                if self.command_palette.take().is_some() {
                    return Task::none();
                }
                let entries = self.palette_entries();
                self.command_palette = Some(CommandPalette::new(entries, &self.recent_commands));
                return iced::widget::operation::focus(command_palette::INPUT_ID);
            }
            CommandId::NewTab => return self.new_tab(None),
            CommandId::NewTabWithShell(i) => {
                if i < self.shells.len() {
                    return self.new_tab(Some(i));
                }
            }
            CommandId::CloseTab => return self.close_tab(self.workspace.active),
            CommandId::ClosePane => {
                if let Some(id) = self.workspace.focused() {
                    return self.close_terminal(id);
                }
            }
            CommandId::NextTab | CommandId::PreviousTab => {
                self.workspace.cycle(id == CommandId::NextTab);
                self.refresh_visible();
            }
            CommandId::SelectTab(i) => {
                if self.workspace.select(i) {
                    self.refresh_visible();
                }
            }
            CommandId::SplitRight => return self.split(Axis::Vertical),
            CommandId::SplitDown => return self.split(Axis::Horizontal),
            CommandId::TogglePaneZoom => {
                if let Some(tab) = self.workspace.active_tab_mut()
                    && tab.panes.len() > 1
                {
                    if tab.panes.maximized().is_some() {
                        tab.panes.restore();
                    } else {
                        tab.panes.maximize(tab.focus);
                    }
                    self.refresh_visible();
                }
            }
            CommandId::FocusPane(dir) => {
                let direction = match dir {
                    Dir::Left => Direction::Left,
                    Dir::Right => Direction::Right,
                    Dir::Up => Direction::Up,
                    Dir::Down => Direction::Down,
                };
                self.restore_zoom();
                if let Some(tab) = self.workspace.active_tab_mut()
                    && tab.focus_adjacent(direction)
                {
                    self.refresh_visible();
                }
            }
            CommandId::RestartShell => {
                let (metrics, palette) = (self.metrics, &self.palette);
                if let Some(focused) = self.workspace.focused()
                    && let Some(t) = self.terminals.get_mut(&focused)
                {
                    return t.restart(metrics, palette);
                }
            }
            CommandId::ClearScrollback
            | CommandId::ScrollToTop
            | CommandId::ScrollToBottom
            | CommandId::ScrollPageUp
            | CommandId::ScrollPageDown => {
                let palette = &self.palette;
                if let Some(focused) = self.workspace.focused()
                    && let Some(t) = self.terminals.get_mut(&focused)
                {
                    if let Some(s) = t.session() {
                        let page = i32::from(t.grid.1.saturating_sub(1).max(1));
                        match id {
                            CommandId::ClearScrollback => s.clear_scrollback(),
                            CommandId::ScrollToTop => s.scroll_to_top(),
                            CommandId::ScrollToBottom => s.scroll_to_bottom(),
                            CommandId::ScrollPageUp => s.scroll(page),
                            _ => s.scroll(-page),
                        }
                    }
                    t.refresh(palette);
                }
            }
            CommandId::Copy => return self.copy_selection(),
            CommandId::Paste => return Task::done(Message::Paste),
            CommandId::ZoomIn => self.set_font_size(self.metrics.font_size + 1.0),
            CommandId::ZoomOut => self.set_font_size(self.metrics.font_size - 1.0),
            CommandId::ZoomReset => self.set_font_size(DEFAULT_FONT_SIZE),
            CommandId::About => self.about_open = true,
            // Same as closing the window: sessions are dropped, shells are shut down.
            CommandId::Quit => return iced::exit(),
        }
        Task::none()
    }

    /// Whether a command can do something right now. The palette only lists
    /// applicable commands, so nothing it shows is a no-op.
    fn is_applicable(&self, id: CommandId) -> bool {
        let focused = self.focused();
        match id {
            CommandId::FocusPane(_) | CommandId::TogglePaneZoom => self.has_splits(),
            CommandId::NextTab | CommandId::PreviousTab => self.workspace.tabs.len() > 1,
            CommandId::Copy => focused
                .and_then(TerminalPane::session)
                .is_some_and(|s| s.selection_text().is_some()),
            CommandId::Paste
            | CommandId::ClearScrollback
            | CommandId::ScrollToTop
            | CommandId::ScrollToBottom
            | CommandId::ScrollPageUp
            | CommandId::ScrollPageDown => focused.is_some_and(TerminalPane::is_running),
            CommandId::RestartShell
            | CommandId::ClosePane
            | CommandId::CloseTab
            | CommandId::SplitRight
            | CommandId::SplitDown => focused.is_some(),
            CommandId::NewTabWithShell(i) => i < self.shells.len(),
            CommandId::SelectTab(i) => i < self.workspace.tabs.len() && i != self.workspace.active,
            CommandId::CommandPalette
            | CommandId::NewTab
            | CommandId::ZoomIn
            | CommandId::ZoomOut
            | CommandId::ZoomReset
            | CommandId::About
            | CommandId::Quit => true,
        }
    }

    /// Display title of a command; parameterised commands use live data.
    fn command_title(&self, id: CommandId) -> String {
        match id {
            CommandId::NewTabWithShell(i) => self.shells.get(i).map_or_else(
                || id.static_title().to_owned(),
                |s| format!("New Tab with {}", s.name),
            ),
            _ => id.static_title().to_owned(),
        }
    }

    /// Entries of menu `index`, resolved against the current state.
    fn menu_rows(&self, index: usize) -> Vec<menu::Row> {
        let Some(m) = self.menus.get(index) else {
            return Vec::new();
        };
        m.items
            .iter()
            .map(|item| match *item {
                menu::Item::Separator => menu::Row::Separator,
                menu::Item::Command(id) => menu::Row::Command {
                    id,
                    label: self.command_title(id),
                    hint: self.keymap.binding_for(id).map(|b| b.to_string()),
                    enabled: self.is_applicable(id),
                },
            })
            .collect()
    }

    fn palette_entries(&self) -> Vec<Entry> {
        let entry = |id: CommandId, title: String| Entry {
            id,
            label: format!("{}: {}", id.category(), title),
            hint: self.keymap.binding_for(id).map(|b| b.to_string()),
        };
        let mut entries: Vec<Entry> = STATIC_COMMANDS
            .iter()
            .map(|&id| entry(id, id.static_title().to_owned()))
            .collect();
        for i in 0..self.shells.len() {
            let id = CommandId::NewTabWithShell(i);
            entries.push(entry(id, self.command_title(id)));
        }
        for (i, tab) in self.workspace.tabs.iter().enumerate() {
            let title = tab
                .focused()
                .and_then(|id| self.terminals.get(&id))
                .map_or_else(|| "—".to_owned(), TerminalPane::display_title);
            entries.push(entry(
                CommandId::SelectTab(i),
                format!("Go to Tab {}: {}", i + 1, title),
            ));
        }
        entries.retain(|e| self.is_applicable(e.id));
        entries
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
        let base: Element<'_, Message> = layout.push(body).push(self.view_status_bar()).into();
        let mut layers = stack![base];
        if let Some(state) = &self.menu {
            let x = menu::dropdown_x(&self.menus, state.open);
            layers = layers.push(menu::view_dropdown(x, &self.menu_rows(state.open), state));
        }
        if let Some(p) = &self.command_palette {
            layers = layers.push(p.view());
        }
        if self.about_open {
            layers = layers.push(view_about());
        }
        layers.into()
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
            .on_press(Message::Command(CommandId::NewTab))
            .padding([2, 9])
            .style(|_, status| ghost_button(status, theme::CYAN));
        let shell_menu = pick_list(choices, None::<ShellChoice>, Message::ShellPicked)
            .placeholder("New tab with…")
            .width(Length::Fixed(150.0))
            .text_size(12)
            .padding([3, 8]);

        let brand = container(
            row![
                svg(svg::Handle::from_memory(LOGO)).width(22).height(22),
                text("ForgeTerm").size(15).color(theme::TEXT),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        )
        .width(Length::Fixed(menu::BRAND_WIDTH));

        container(
            row![
                brand,
                menu::view_titles(&self.menus, self.menu.as_ref()),
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
        .padding([0, 14])
        .height(Length::Fixed(menu::BAR_HEIGHT))
        .align_y(iced::Alignment::Center)
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
                "Tab {}/{} · {} pane{}{}",
                self.workspace.active + 1,
                self.workspace.tabs.len(),
                tab.panes.len(),
                if tab.panes.len() == 1 { "" } else { "s" },
                if tab.panes.maximized().is_some() {
                    " · zoomed (Ctrl+Shift+Enter)"
                } else {
                    ""
                }
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

/// Help → About: version, creator and the open-source components ForgeTerm is built on.
fn view_about<'a>() -> Element<'a, Message> {
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
