//! Application state, update loop and layout: tabs of split terminal panes.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use forgeterm_agents::{Agent, AgentKind, ProjectContext};
use forgeterm_explorer::{Listing, Tree, list_dir};
use forgeterm_settings::Settings;
use forgeterm_terminal::{
    Palette, SessionEvent, ShellSpec, detect_shells, encode_key, paste_needs_confirmation,
    quote_path,
};
use iced::keyboard::{self, key::Named};
use iced::widget::canvas::Canvas;
use iced::widget::pane_grid::{self, Axis, Direction, PaneGrid};
use iced::widget::{
    button, column, container, mouse_area, opaque, pick_list, row, scrollable, space, stack, svg,
    text,
};
use iced::{
    Border, Color, Element, Event, Length, Shadow, Subscription, Task, Vector, clipboard, event,
    window,
};

use crate::ai_view::{self, AgentState};
use crate::commands::{CommandId, Dir, Keymap, STATIC_COMMANDS};
use crate::menu::{self, Menu, MenuState};
use crate::palette::{self as command_palette, CommandPalette, Entry};
use crate::terminal_pane::{Outcome, SessionOptions, Status, TermId, TermMsg, TerminalPane};
use crate::terminal_view::{Metrics, TerminalCanvas};
use crate::workspace::Workspace;
use crate::{explorer_view, keys, theme};

const LOGO: &[u8] = include_bytes!("../../../assets/branding/forgeterm-logo.svg");
const MAX_TAB_TITLE: usize = 28;
/// The horizontally scrolling tab strip.
const TABS_ID: &str = "forgeterm-tabs";

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
    ExplorerListed(PathBuf, Result<Listing, String>),
    ExplorerSelect(PathBuf),
    /// Chevron click: expand / collapse a folder.
    ExplorerToggle(PathBuf),
    /// Double click: folders expand / collapse, files are inserted as a path.
    ExplorerActivate(PathBuf, bool),
    /// Files dragged over the window from the OS (shows a drop hint).
    FileHovered,
    FilesHoveredLeft,
    FileDropped(PathBuf),
    /// Periodic check whether settings.toml changed on disk.
    SettingsTick,
    DismissSettingsProblems,
    AgentsDetected(Vec<(AgentKind, Option<Agent>)>),
    /// Project context scanned for the given start folder.
    ContextScanned(PathBuf, ProjectContext),
    /// Insert a project file into the focused terminal (agent: `@path`).
    AiInsert(PathBuf),
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
    explorer: Tree,
    explorer_visible: bool,
    /// The explorer follows the focused shell's directory.
    explorer_follow: bool,
    /// Shell directory the explorer was last moved to; manual navigation in the
    /// explorer stays until the shell reports a different directory.
    explorer_synced: Option<PathBuf>,
    /// (active tab, tab count) the tab strip was last scrolled for.
    revealed_tab: (usize, usize),
    /// Files from the OS are being dragged over the window.
    drop_hover: bool,
    settings: Settings,
    /// `None` when no config directory could be determined.
    settings_path: Option<PathBuf>,
    /// Modification time of the settings file when it was last loaded.
    settings_mtime: Option<SystemTime>,
    /// Problems from the last load, shown until dismissed.
    settings_problems: Vec<String>,
    /// Where the first tab starts and the fallback for unknown shell directories.
    start_dir: PathBuf,
    ai_visible: bool,
    agents: Vec<(AgentKind, AgentState)>,
    ai_context: Option<ProjectContext>,
    /// Folder the context was (or is being) scanned for.
    ai_context_for: Option<PathBuf>,
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let mut app = Self {
            terminals: HashMap::new(),
            workspace: Workspace::default(),
            next_id: 1,
            shells: detect_shells(),
            palette: Palette::default(),
            metrics: Metrics::new(Settings::default().terminal.font_size),
            pending_paste: None,
            error: None,
            keymap: Keymap::default(),
            command_palette: None,
            recent_commands: Vec::new(),
            menus: Vec::new(),
            menu: None,
            about_open: false,
            explorer: Tree::new(launch_dir()),
            explorer_visible: false,
            explorer_follow: true,
            explorer_synced: None,
            revealed_tab: (0, 0),
            drop_hover: false,
            settings: Settings::default(),
            settings_path: forgeterm_settings::default_path(),
            settings_mtime: None,
            settings_problems: Vec::new(),
            start_dir: launch_dir(),
            ai_visible: false,
            agents: AgentKind::ALL
                .iter()
                .map(|k| (*k, AgentState::Detecting))
                .collect(),
            ai_context: None,
            ai_context_for: None,
        };
        app.menus = menu::menus(app.shells.len());
        app.load_settings(true);
        let terminal = app.new_tab(None, None);
        let root = app.explorer.root().to_path_buf();
        (
            app,
            Task::batch([terminal, load_dir(root), detect_agents()]),
        )
    }

    // ---- terminal lifecycle -------------------------------------------------------

    /// Starts a terminal in `cwd` (default: the directory ForgeTerm started in).
    fn spawn_terminal(
        &mut self,
        shell: ShellSpec,
        cwd: Option<PathBuf>,
    ) -> (TermId, Task<Message>) {
        self.spawn_terminal_for(shell, cwd, None)
    }

    /// Like [`Self::spawn_terminal`]; agents run without shell integration.
    fn spawn_terminal_for(
        &mut self,
        shell: ShellSpec,
        cwd: Option<PathBuf>,
        agent: Option<AgentKind>,
    ) -> (TermId, Task<Message>) {
        let id = self.next_id;
        self.next_id += 1;
        let cwd = Some(cwd.unwrap_or_else(|| self.start_dir.clone()));
        let options = SessionOptions {
            scrollback: self.settings.terminal.scrollback,
            shell_integration: agent.is_none() && self.settings.terminal.shell_integration,
        };
        let (mut pane, task) =
            TerminalPane::new(id, shell, cwd, options, self.metrics, &self.palette);
        pane.agent = agent;
        self.terminals.insert(id, pane);
        (id, task)
    }

    fn shell_at(&self, index: Option<usize>) -> Option<ShellSpec> {
        match index {
            Some(i) => self.shells.get(i).cloned(),
            None => self
                .default_shell_index()
                .and_then(|i| self.shells.get(i).cloned()),
        }
    }

    /// The configured default shell, or the first detected one.
    fn default_shell_index(&self) -> Option<usize> {
        let wanted = self.settings.terminal.default_shell.as_deref();
        wanted
            .and_then(|name| {
                self.shells
                    .iter()
                    .position(|s| s.name.eq_ignore_ascii_case(name))
            })
            .or_else(|| (!self.shells.is_empty()).then_some(0))
    }

    // ---- settings -------------------------------------------------------------------

    /// (Re)loads settings.toml and applies what changed. At startup everything is
    /// applied; on reload only settings whose value in the file changed, so runtime
    /// toggles (zoom, explorer) are not reset by unrelated edits.
    fn load_settings(&mut self, startup: bool) {
        let Some(path) = self.settings_path.clone() else {
            return;
        };
        self.settings_mtime = file_mtime(&path);
        let loaded = forgeterm_settings::load(&path);
        // A half-edited file with a syntax error must not reset a running app to
        // defaults: keep the current settings and only report the error.
        if loaded.unreadable && !startup {
            self.settings_problems = loaded
                .problems
                .into_iter()
                .map(|p| format!("{p} (current settings kept)"))
                .collect();
            return;
        }
        let mut problems = loaded.problems;
        let new = loaded.settings;
        let old = std::mem::replace(&mut self.settings, new.clone());

        if let Some(name) = &new.terminal.default_shell
            && !self
                .shells
                .iter()
                .any(|s| s.name.eq_ignore_ascii_case(name))
        {
            let available: Vec<&str> = self.shells.iter().map(|s| s.name.as_str()).collect();
            problems.push(format!(
                "terminal.default_shell = \"{name}\" is not installed; available: {}",
                available.join(", ")
            ));
        }
        self.start_dir = match &new.terminal.start_directory {
            Some(dir) if dir.is_dir() => dir.clone(),
            Some(dir) => {
                problems.push(format!(
                    "terminal.start_directory = \"{}\" is not a folder; using {}",
                    dir.display(),
                    launch_dir().display()
                ));
                launch_dir()
            }
            None => launch_dir(),
        };

        let (t_old, t_new) = (&old.terminal, &new.terminal);
        if startup || t_old.font_size != t_new.font_size {
            self.set_font_size(t_new.font_size);
        }
        let (e_old, e_new) = (&old.explorer, &new.explorer);
        if startup || e_old.visible != e_new.visible {
            self.explorer_visible = e_new.visible;
        }
        if startup || e_old.follow_terminal != e_new.follow_terminal {
            self.explorer_follow = e_new.follow_terminal;
            self.explorer_synced = None;
        }
        if startup || e_old.show_hidden != e_new.show_hidden {
            self.explorer.set_show_hidden(e_new.show_hidden);
        }
        if startup && self.explorer.root() != self.start_dir {
            self.explorer.set_root(self.start_dir.clone());
        }
        for p in &problems {
            tracing::warn!("settings: {p}");
        }
        self.settings_problems = problems;
    }

    fn settings_changed_on_disk(&self) -> bool {
        self.settings_path
            .as_deref()
            .is_some_and(|p| file_mtime(p) != self.settings_mtime)
    }

    fn new_tab(&mut self, shell_index: Option<usize>, cwd: Option<PathBuf>) -> Task<Message> {
        let Some(shell) = self.shell_at(shell_index) else {
            self.error = Some("No supported shell was found on this system.".into());
            return Task::none();
        };
        let cwd = cwd.or_else(|| self.focused_cwd());
        let (id, task) = self.spawn_terminal(shell, cwd);
        self.workspace.push_tab(id);
        task
    }

    fn split(&mut self, axis: Axis) -> Task<Message> {
        // A split starts the same shell as the pane it splits.
        let shell = self
            .workspace
            .focused()
            .and_then(|id| self.terminals.get(&id))
            .filter(|t| t.agent.is_none())
            .map(|t| t.shell.clone())
            .or_else(|| self.shell_at(None));
        let Some(shell) = shell else {
            return Task::none();
        };
        self.restore_zoom();
        let cwd = self.focused_cwd();
        let (id, task) = self.spawn_terminal(shell, cwd);
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

    /// Directory of the focused terminal (see [`TerminalPane::directory`]).
    fn focused_cwd(&self) -> Option<PathBuf> {
        self.focused()
            .and_then(TerminalPane::directory)
            .map(Path::to_path_buf)
    }

    /// Moves the explorer to the focused shell's directory when that directory (or
    /// the focused shell) changed since the last sync.
    fn sync_explorer(&mut self) -> Option<Task<Message>> {
        if !self.explorer_visible || !self.explorer_follow {
            return None;
        }
        let cwd = self.focused_cwd()?;
        if self.explorer_synced.as_ref() == Some(&cwd) {
            return None;
        }
        self.explorer_synced = Some(cwd.clone());
        if self.explorer.root() == cwd {
            return None;
        }
        Some(load_dir(self.explorer.set_root(cwd)))
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
        // A cheap stat every 2 s picks up saved edits to settings.toml.
        let settings = iced::time::every(Duration::from_secs(2)).map(|_| Message::SettingsTick);
        let events = event::listen_with(|event, status, _window| match event {
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
            Event::Window(window::Event::FileHovered(_)) => Some(Message::FileHovered),
            Event::Window(window::Event::FilesHoveredLeft) => Some(Message::FilesHoveredLeft),
            Event::Window(window::Event::FileDropped(path)) => Some(Message::FileDropped(path)),
            _ => None,
        });
        Subscription::batch([events, settings])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.handle(message);
        let follow_ups: Vec<Task<Message>> = [
            self.sync_explorer(),
            self.reveal_active_tab(),
            self.sync_ai_context(),
        ]
        .into_iter()
        .flatten()
        .collect();
        if follow_ups.is_empty() {
            task
        } else {
            Task::batch(std::iter::once(task).chain(follow_ups))
        }
    }

    /// Rescans the project context when the AI panel is visible and the focused
    /// shell's folder changed.
    fn sync_ai_context(&mut self) -> Option<Task<Message>> {
        if !self.ai_visible {
            return None;
        }
        let dir = self.focused_cwd().unwrap_or_else(|| self.start_dir.clone());
        if self.ai_context_for.as_ref() == Some(&dir) {
            return None;
        }
        self.ai_context_for = Some(dir.clone());
        Some(scan_context(dir))
    }

    fn found_agent(&self, kind: AgentKind) -> Option<&Agent> {
        self.agents.iter().find_map(|(k, s)| match s {
            AgentState::Found(a) if *k == kind => Some(a),
            _ => None,
        })
    }

    /// Starts an agent in the focused shell's project root, as a new tab or split.
    fn start_agent(&mut self, kind: AgentKind, split: bool) -> Task<Message> {
        let Some(agent) = self.found_agent(kind) else {
            return Task::none();
        };
        let launch = forgeterm_agents::launch_command(&agent.path);
        let shell = ShellSpec::new(kind.name(), launch.program, launch.args);
        let here = self.focused_cwd().unwrap_or_else(|| self.start_dir.clone());
        let root = forgeterm_agents::project_root(&here).0;
        if split {
            self.restore_zoom();
        }
        let (id, task) = self.spawn_terminal_for(shell, Some(root), Some(kind));
        if split {
            let ok = self
                .workspace
                .active_tab_mut()
                .and_then(|tab| tab.split(Axis::Vertical, id))
                .is_some();
            if !ok {
                self.terminals.remove(&id);
                return Task::none();
            }
            self.refresh_visible();
        } else {
            self.workspace.push_tab(id);
        }
        task
    }

    /// Scrolls the tab strip so the active tab is visible after it changed.
    /// Approximate (tabs differ in width): first tab = start, last = end.
    fn reveal_active_tab(&mut self) -> Option<Task<Message>> {
        let key = (self.workspace.active, self.workspace.tabs.len());
        if self.revealed_tab == key {
            return None;
        }
        self.revealed_tab = key;
        let x = if key.1 <= 1 {
            0.0
        } else {
            key.0 as f32 / (key.1 - 1) as f32
        };
        Some(iced::widget::operation::snap_to(
            TABS_ID,
            iced::widget::operation::RelativeOffset { x, y: 0.0 },
        ))
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
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
                    // Picked up by `sync_explorer` after this message.
                    Outcome::CwdChanged => {}
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
            Message::ClipboardText(Some(text)) => self.insert_text(text),
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
            Message::ExplorerListed(path, result) => self.explorer.set_listing(path, result),
            Message::ExplorerSelect(path) => self.explorer.select(Some(path)),
            Message::ExplorerToggle(path) => {
                self.explorer.select(Some(path.clone()));
                return self
                    .explorer
                    .toggle(&path)
                    .map_or_else(Task::none, load_dir);
            }
            Message::ExplorerActivate(path, true) => {
                return self
                    .explorer
                    .toggle(&path)
                    .map_or_else(Task::none, load_dir);
            }
            Message::ExplorerActivate(path, false) => self.insert_paths(&[path]),
            Message::FileHovered => self.drop_hover = true,
            Message::FilesHoveredLeft => self.drop_hover = false,
            Message::SettingsTick => {
                if self.settings_changed_on_disk() {
                    self.load_settings(false);
                }
            }
            Message::DismissSettingsProblems => self.settings_problems.clear(),
            Message::AgentsDetected(found) => {
                self.agents = found
                    .into_iter()
                    .map(|(k, a)| (k, a.map_or(AgentState::Missing, AgentState::Found)))
                    .collect();
            }
            Message::ContextScanned(dir, ctx) => {
                if self.ai_context_for.as_ref() == Some(&dir) {
                    self.ai_context = Some(ctx);
                }
            }
            Message::AiInsert(path) => self.insert_context_file(&path),
            Message::FileDropped(path) => {
                self.drop_hover = false;
                self.insert_paths(&[path]);
            }
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
            CommandId::NewTab => return self.new_tab(None, None),
            CommandId::NewTabWithShell(i) => {
                if i < self.shells.len() {
                    return self.new_tab(Some(i), None);
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
            CommandId::ZoomReset => self.set_font_size(self.settings.terminal.font_size),
            CommandId::OpenSettings => {
                if let Some(path) = self.settings_path.clone() {
                    let opened = forgeterm_settings::ensure_file(&path)
                        .map_err(|e| format!("could not create {}: {e}", path.display()))
                        .and_then(|()| open_in_editor(&path));
                    if let Err(e) = opened {
                        self.settings_problems = vec![e];
                    }
                }
            }
            CommandId::ReloadSettings => self.load_settings(false),
            CommandId::ToggleExplorer => {
                self.explorer_visible = !self.explorer_visible;
                if self.explorer_visible {
                    self.ai_visible = false;
                }
            }
            CommandId::ToggleAiPanel => {
                self.ai_visible = !self.ai_visible;
                if self.ai_visible {
                    self.explorer_visible = false;
                }
            }
            CommandId::AiRefresh => {
                self.agents = AgentKind::ALL
                    .iter()
                    .map(|k| (*k, AgentState::Detecting))
                    .collect();
                self.ai_context_for = None;
                return detect_agents();
            }
            CommandId::StartAgent(kind, split) => return self.start_agent(kind, split),
            CommandId::ExplorerToggleFollow => {
                self.explorer_follow = !self.explorer_follow;
                // Turning it on jumps to the shell's directory right away.
                self.explorer_synced = None;
            }
            CommandId::ExplorerToggleHidden => {
                let show = !self.explorer.show_hidden();
                self.explorer.set_show_hidden(show);
            }
            CommandId::ExplorerRefresh => {
                return Task::batch(self.explorer.refresh().into_iter().map(load_dir));
            }
            CommandId::ExplorerParent => {
                if let Some(parent) = self.explorer.parent() {
                    return load_dir(self.explorer.set_root(parent));
                }
            }
            CommandId::ExplorerOpenSelected => {
                if let Some(dir) = self
                    .explorer
                    .selected()
                    .filter(|e| e.is_dir)
                    .map(|e| e.path.clone())
                {
                    return load_dir(self.explorer.set_root(dir));
                }
            }
            CommandId::InsertSelectedPath => {
                if let Some(path) = self.explorer.selected().map(|e| e.path.clone()) {
                    self.insert_paths(&[path]);
                }
            }
            CommandId::CopySelectedPath => {
                if let Some(e) = self.explorer.selected() {
                    return clipboard::write(e.path.to_string_lossy().into_owned());
                }
            }
            CommandId::NewTabInSelectedFolder => {
                let dir = self.explorer.selected().map(|e| {
                    if e.is_dir {
                        e.path.clone()
                    } else {
                        e.path
                            .parent()
                            .map_or_else(|| e.path.clone(), Path::to_path_buf)
                    }
                });
                if let Some(dir) = dir {
                    return self.new_tab(None, Some(dir));
                }
            }
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
            | CommandId::Quit
            | CommandId::ToggleExplorer
            | CommandId::ToggleAiPanel
            | CommandId::AiRefresh => true,
            CommandId::StartAgent(kind, _) => self.found_agent(kind).is_some(),
            CommandId::OpenSettings | CommandId::ReloadSettings => self.settings_path.is_some(),
            CommandId::ExplorerToggleHidden
            | CommandId::ExplorerRefresh
            | CommandId::ExplorerToggleFollow => self.explorer_visible,
            CommandId::ExplorerParent => self.explorer_visible && self.explorer.parent().is_some(),
            CommandId::ExplorerOpenSelected => {
                self.explorer_visible && self.explorer.selected().is_some_and(|e| e.is_dir)
            }
            CommandId::CopySelectedPath | CommandId::NewTabInSelectedFolder => {
                self.explorer_visible && self.explorer.selected().is_some()
            }
            CommandId::InsertSelectedPath => {
                self.explorer_visible
                    && self.explorer.selected().is_some()
                    && focused.is_some_and(TerminalPane::is_running)
            }
        }
    }

    /// Inserts text at the focused terminal's prompt like a paste: never followed by
    /// Enter, and multi-line text without bracketed paste asks for confirmation.
    fn insert_text(&mut self, text: String) {
        let Some(t) = self.focused() else {
            return;
        };
        if paste_needs_confirmation(&text, t.modes.bracketed_paste) {
            self.pending_paste = Some((t.id, text));
        } else if let Some(s) = t.session() {
            s.paste(&text);
        }
    }

    /// Inserts paths quoted for the focused terminal's shell, followed by a space.
    /// Inserts a project file into the focused terminal: as an `@path` file
    /// reference (relative to the project root) when it runs Claude Code, otherwise
    /// as a path quoted for the shell. Never followed by Enter.
    fn insert_context_file(&mut self, path: &Path) {
        let agent = self.focused().and_then(|t| t.agent);
        match agent {
            Some(AgentKind::ClaudeCode) => {
                let root = self.ai_context.as_ref().map(|c| c.root.clone());
                let rel = root
                    .as_deref()
                    .and_then(|r| path.strip_prefix(r).ok())
                    .map_or_else(|| path.to_path_buf(), Path::to_path_buf);
                let text = rel.to_string_lossy().replace('\\', "/");
                // A reference with spaces would end at the first space.
                if text.contains(char::is_whitespace) {
                    self.insert_text(format!("{} ", path.display()));
                } else {
                    self.insert_text(format!("@{text} "));
                }
            }
            Some(AgentKind::Codex) => self.insert_text(format!("{} ", path.display())),
            None => self.insert_paths(&[path.to_path_buf()]),
        }
    }

    fn insert_paths(&mut self, paths: &[PathBuf]) {
        let Some(kind) = self.focused().map(|t| t.shell.kind) else {
            return;
        };
        let mut text = String::new();
        for p in paths {
            text.push_str(&quote_path(kind, &p.to_string_lossy()));
            text.push(' ');
        }
        self.insert_text(text);
    }

    /// Display title of a command; parameterised commands use live data.
    fn command_title(&self, id: CommandId) -> String {
        match id {
            CommandId::NewTabWithShell(i) => self.shells.get(i).map_or_else(
                || id.static_title().to_owned(),
                |s| format!("New Tab with {}", s.name),
            ),
            CommandId::StartAgent(kind, false) => format!("Start {kind} in Project"),
            CommandId::StartAgent(kind, true) => format!("Start {kind} in Split"),
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
        for kind in AgentKind::ALL {
            for split in [false, true] {
                let id = CommandId::StartAgent(kind, split);
                entries.push(entry(id, self.command_title(id)));
            }
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
        if !self.settings_problems.is_empty() {
            layout = layout.push(settings_banner(&self.settings_problems));
        }
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
        let body: Element<'_, Message> = if self.explorer_visible {
            row![
                explorer_view::view(&self.explorer, self.explorer_follow),
                body
            ]
            .into()
        } else if self.ai_visible {
            row![ai_view::view(&self.agents, self.ai_context.as_ref()), body].into()
        } else {
            body
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
        if self.drop_hover {
            let shell = self
                .focused()
                .map_or("the terminal", |t| t.shell.name.as_str());
            layers = layers.push(drop_hint(shell));
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
                // Many tabs scroll horizontally (wheel / trackpad) instead of pushing
                // the branding out of the window.
                scrollable(
                    row![tabs, new_tab, shell_menu]
                        .spacing(8)
                        .align_y(iced::Alignment::Center)
                )
                .id(TABS_ID)
                .direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::new().width(0).scroller_width(0),
                ))
                .width(Length::Fill),
                text("Erik van de Blaak Edition")
                    .size(12)
                    .color(theme::TEXT_MUTED)
                    .wrapping(text::Wrapping::None),
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
        let cwd = focused
            .and_then(TerminalPane::directory)
            .map_or_else(String::new, short_path);
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
                text(cwd)
                    .size(12)
                    .color(theme::TEXT)
                    .wrapping(text::Wrapping::None),
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

/// Status-bar form of a directory: home as `~`, long paths cut at the front.
fn short_path(path: &Path) -> String {
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

/// Shown while files from the OS are dragged over the window.
fn drop_hint(shell: &str) -> Element<'_, Message> {
    let card = container(
        column![
            text("Drop to insert the path").size(16).color(theme::TEXT),
            text(format!(
                "Quoted for {shell} · inserted at the prompt, not executed"
            ))
            .size(12)
            .color(theme::TEXT_MUTED),
        ]
        .spacing(4)
        .align_x(iced::Alignment::Center),
    )
    .padding([16, 24])
    .style(|_| container::Style {
        background: Some(theme::SURFACE.into()),
        border: Border {
            color: theme::CYAN,
            width: 2.0,
            radius: 10.0.into(),
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

/// The folder ForgeTerm was started from.
fn launch_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Opens a text file for editing (detached). Windows: Notepad, which is always
/// present (`.toml` usually has no file association, which would only show the
/// "How do you want to open this file?" dialog). Elsewhere: the desktop's default.
fn open_in_editor(path: &Path) -> Result<(), String> {
    let program = if cfg!(windows) {
        "notepad.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(path)
        .spawn()
        .map(drop)
        .map_err(|e| format!("could not open {} with {program}: {e}", path.display()))
}

/// Problems found in settings.toml (each setting fell back to its default).
fn settings_banner(problems: &[String]) -> Element<'_, Message> {
    const SHOWN: usize = 3;
    let mut lines = column![
        text(format!(
            "settings.toml: {} problem{}",
            problems.len(),
            if problems.len() == 1 { "" } else { "s" }
        ))
        .size(13)
        .color(theme::TEXT)
    ]
    .spacing(2);
    for p in problems.iter().take(SHOWN) {
        lines = lines.push(text(format!("• {p}")).size(12).color(theme::TEXT_MUTED));
    }
    if problems.len() > SHOWN {
        lines = lines.push(
            text(format!(
                "… and {} more (see the log)",
                problems.len() - SHOWN
            ))
            .size(12)
            .color(theme::TEXT_MUTED),
        );
    }
    let action = |label: &'static str, msg: Message| {
        button(text(label).size(12))
            .on_press(msg)
            .padding([5, 10])
            .style(|_, status| ghost_button(status, theme::CYAN))
    };
    container(
        row![
            text("⚙").size(16).color(theme::AMBER),
            lines,
            space::horizontal(),
            action("Open settings", Message::Command(CommandId::OpenSettings)),
            action("Dismiss", Message::DismissSettingsProblems),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center),
    )
    .padding([8, 14])
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(theme::SURFACE.into()),
        border: Border {
            color: theme::AMBER,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

/// Lists a directory on a background thread (slow or network drives must never
/// block the UI) and reports the result.
/// Runs `f` on a background thread and maps its result to a message.
fn background<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
    to_message: impl Fn(T) -> Message + Send + 'static,
) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(f());
            });
            rx.await
        },
        move |result| match result {
            Ok(v) => to_message(v),
            // The worker thread panicked; nothing to report beyond the log.
            Err(_) => Message::DismissSettingsProblems,
        },
    )
}

/// Locates the agents and asks each for its version (off the UI thread; a slow
/// virus scan of the executable must not freeze the window).
fn detect_agents() -> Task<Message> {
    background(
        || {
            AgentKind::ALL
                .iter()
                .map(|&kind| {
                    let agent = forgeterm_agents::locate(kind).map(|path| Agent {
                        kind,
                        version: forgeterm_agents::query_version(
                            &path,
                            std::time::Duration::from_secs(20),
                        ),
                        path,
                    });
                    (kind, agent)
                })
                .collect()
        },
        Message::AgentsDetected,
    )
}

fn scan_context(dir: PathBuf) -> Task<Message> {
    let key = dir.clone();
    background(
        move || forgeterm_agents::scan_context(&dir),
        move |ctx| Message::ContextScanned(key.clone(), ctx),
    )
}

fn load_dir(path: PathBuf) -> Task<Message> {
    let target = path.clone();
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(list_dir(&target));
            });
            rx.await
                .unwrap_or_else(|_| Err("listing was interrupted".to_owned()))
        },
        move |result| Message::ExplorerListed(path.clone(), result),
    )
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
