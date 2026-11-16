//! Application state, update loop and layout: tabs of split terminal panes.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use forgeterm_agents::{Agent, AgentKind, ProjectContext};
use forgeterm_explorer::{Listing, Tree, list_dir};
use forgeterm_settings::Settings;
use forgeterm_terminal::{
    Palette, SessionEvent, ShellSpec, detect_shells, paste_needs_confirmation, quote_path,
};
use iced::keyboard::{self, key::Named};
use iced::widget::canvas::Canvas;
use iced::widget::pane_grid::{self, Axis, Direction, PaneGrid};
use iced::widget::{
    button, column, container, mouse_area, opaque, pick_list, row, scrollable, space, stack, svg,
    text, text_input,
};
use iced::{
    Border, Color, Element, Event, Length, Shadow, Subscription, Task, Vector, clipboard, event,
    window,
};

use crate::ai_view::{self, AgentState};
use crate::commands::{CommandId, Dir, Keymap, STATIC_COMMANDS};
use crate::compare_view::{self, Compare};
use crate::git_dash::{self, DashData, GitDash, WorktreeChanges, WorktreeInfo};
use crate::git_view::{self, DiffView, GitState, HistoryView, Repo, Snapshot};
use crate::menu::{self, Menu, MenuState};
use crate::mission_view::{self, EndKind, Ended, Mission};
use crate::monitor_view::{self, Monitor};
use crate::palette::{self as command_palette, CommandPalette, Entry};
use crate::prompt_studio::PromptStudio;
use crate::terminal_pane::{Outcome, SessionOptions, Status, TermId, TermMsg, TerminalPane};
use crate::terminal_view::{Metrics, TerminalCanvas};
use crate::viewer_view::{self, Viewer, ViewerMsg, ViewerTab};
use crate::workspace::Workspace;
use crate::worktree_form::{self, WorktreeForm};
use crate::{explorer_view, keys, theme};

const LOGO: &[u8] = include_bytes!("../../../assets/branding/forgeterm-logo.svg");
/// Start of the notice when git refuses to remove a worktree.
const REMOVE_FAILED: &str = "git did not remove the worktree: ";
const MAX_TAB_TITLE: usize = 28;
/// The horizontally scrolling tab strip.
const TABS_ID: &str = "forgeterm-tabs";
pub const EXPLORER_LIST_ID: &str = "forgeterm-explorer-list";
pub const EXPLORER_GOTO_ID: &str = "forgeterm-explorer-goto";

/// Navigation keys of the focused explorer (no modifiers except Shift-less).
fn explorer_key(key: &keyboard::Key, mods: keyboard::Modifiers) -> Option<forgeterm_explorer::Nav> {
    use forgeterm_explorer::Nav;
    if mods.control() || mods.alt() || mods.shift() {
        return None;
    }
    let keyboard::Key::Named(named) = key else {
        return None;
    };
    Some(match named {
        Named::ArrowUp => Nav::Up,
        Named::ArrowDown => Nav::Down,
        Named::ArrowLeft => Nav::Left,
        Named::ArrowRight => Nav::Right,
        Named::PageUp => Nav::PageUp,
        Named::PageDown => Nav::PageDown,
        Named::Home => Nav::Home,
        Named::End => Nav::End,
        Named::Enter => Nav::Activate,
        _ => return None,
    })
}

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
    ShortcutsClose,
    /// A Ctrl+clicked file reference was looked up (`found`: the local file).
    FileRefResolved {
        wanted: String,
        line: Option<u32>,
        found: Option<PathBuf>,
    },
    /// The window gained (`true`) or lost keyboard focus.
    WindowFocus(bool),
    ExplorerListed(PathBuf, Result<Listing, String>),
    /// Every 2 s while the explorer is visible: look for changed folders.
    ExplorerTick,
    ExplorerChanged(Vec<PathBuf>),
    ExplorerScrolled(scrollable::Viewport),
    ExplorerGoToInput(String),
    ExplorerGoToSubmit,
    ExplorerGoToResult(Result<PathBuf, String>),
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
    /// A background task panicked (see `background`).
    WorkerPanicked,
    AgentsDetected(Vec<(AgentKind, Option<Agent>)>),
    /// Project context scanned for the given start folder.
    ContextScanned(PathBuf, ProjectContext),
    /// Insert a project file into the focused terminal (agent: `@path`).
    AiInsert(PathBuf),
    PromptsLoaded(forgeterm_prompts::Library),
    PromptSelect(usize),
    PromptEdit(String, iced::widget::text_editor::Action),
    PromptClose,
    /// Periodic git status refresh.
    GitTick,
    GitRefreshed(Box<Snapshot>),
    GitStage(Vec<String>),
    GitUnstage(Vec<String>),
    GitCommitEdit(iced::widget::text_editor::Action),
    /// Result of stage / unstage / commit (message for the panel).
    GitActionDone(Result<String, String>),
    GitShowDiff(String, forgeterm_git::DiffKind),
    GitDiffLoaded(
        String,
        forgeterm_git::DiffKind,
        Result<forgeterm_git::Diff, String>,
    ),
    GitDiffClose,
    GitHistoryLoaded(Box<HistoryData>),
    GitSelectCommit(String),
    GitCommitLoaded(String, Result<forgeterm_git::Diff, String>),
    GitHistoryClose,
    /// "Switch" clicked: ask for confirmation first.
    GitSwitchAsk(String),
    GitSwitchCancel,
    GitSwitchConfirm,
    GitTrack(String),
    GitNewBranchName(String),
    GitCreateBranch,
    /// "Open in viewer" in the diff overlay (path relative to the repository root).
    GitOpenInViewer(String),
    /// Trust a repository whose own config names programs (session only).
    Viewer(ViewerMsg),
    /// The window's close button.
    WindowCloseRequested,
    /// Programs found running in the terminals about to be closed.
    CloseChecked(CloseTarget, Vec<String>),
    ConfirmClose,
    CancelClose,
    TermFindQuery(String),
    /// The typing pause after keystroke `n` ended (see [`TERM_FIND_DEBOUNCE`]).
    TermFindDue(u64),
    /// Search again: `true` = older output (up), `false` = newer (down).
    TermFindStep(bool),
    TermFindClose,
    DismissNotice,
    MonitorSampled(Box<forgeterm_system::Sample>),
    /// Once per second while Mission Control is open (runtimes, branch refresh).
    MissionTick,
    WorktreeFormAgent(AgentKind),
    WorktreeFormBranch(String),
    /// The repository's branch names, read when the form opened.
    WorktreeFormBranches(Vec<String>),
    WorktreeFormSubmit,
    WorktreeFormCancel,
    /// `git worktree add` finished: the new folder, or git's error.
    WorktreeCreated(Result<PathBuf, String>),
    WorktreeRemoveConfirm,
    WorktreeRemoveCancel,
    WorktreeRemoved(Result<(), String>),
    /// Compare Worktrees data, read in the background.
    CompareLoaded(Box<compare_view::CompareData>),
    ComparePickA(compare_view::Choice),
    ComparePickB(compare_view::Choice),
    CompareSelect(String),
    /// (the A/B folders it was read from, path, side-by-side rows and whether
    /// git's diff was cut off).
    CompareDiffLoaded(
        [PathBuf; 2],
        String,
        Result<(forgeterm_git::SideBySide, bool), String>,
    ),
    CompareTestCommand(String),
    ComparePromptEdit(iced::widget::text_editor::Action),
    /// Twice a second while Compare is open or a test command waits for a prompt.
    CompareTick,
    /// Git Control Center data, read in the background.
    GitDashLoaded(Box<DashData>),
    /// Checked-out branch per project root, read in the background.
    MissionHeads(Vec<(PathBuf, Option<forgeterm_git::Head>)>),
    /// Test hook (FORGETERM_SCREENSHOT): capture the rendered frame.
    DebugScreenshot,
    DebugScreenshotTaken(iced::window::Screenshot),
}

const TERM_FIND_ID: &str = "forgeterm-terminal-find";

/// The side panels beside the terminals; at most one is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidePanel {
    Explorer,
    Ai,
    Git,
    Monitor,
}

/// The terminal search bar: query and the outcome of the last search.
#[derive(Debug, Default)]
struct TermFind {
    query: String,
    outcome: Option<forgeterm_terminal::SearchOutcome>,
    /// Bumped per keystroke: only the search for the last one runs.
    seq: u64,
    /// The current query has been searched (else a debounced search is pending).
    searched: bool,
}

/// Typing pause before the terminal is searched: a miss scans the whole
/// scrollback under the terminal lock (8.8 ms per 10k lines, docs/PERFORMANCE.md).
const TERM_FIND_DEBOUNCE: Duration = Duration::from_millis(150);

/// What a close action would end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseTarget {
    Quit,
    /// All terminals of a tab (ids, so the index may change meanwhile).
    Tab(Vec<TermId>),
    Pane(TermId),
}

/// History overlay data for one repository root.
#[derive(Debug, Clone)]
pub struct HistoryData {
    root: PathBuf,
    entries: Result<Vec<forgeterm_git::HistoryEntry>, String>,
    activity: Result<forgeterm_git::Activity, String>,
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
    /// The window has keyboard focus (attention for unseen terminals otherwise).
    window_focused: bool,
    explorer: Tree,
    /// The explorer has the keyboard (after a click in it or Ctrl+Shift+E).
    explorer_focused: bool,
    /// "Go to Folder" field: text and the last problem.
    explorer_goto: Option<(String, Option<String>)>,
    /// Explorer list scroll offset and visible height (for keeping the selection
    /// in view).
    explorer_scroll: (f32, f32),
    /// A change check is running (no second one meanwhile).
    explorer_checking: bool,
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
    agents: Vec<(AgentKind, AgentState)>,
    ai_context: Option<ProjectContext>,
    /// Folder the context was (or is being) scanned for.
    ai_context_for: Option<PathBuf>,
    prompt_studio: Option<PromptStudio>,
    /// Prompt Studio is shown beside the terminal instead of as a floating window.
    prompt_docked: bool,
    /// The docked studio has the keyboard (set by editing in it; Esc or a click in a
    /// terminal gives the keyboard back to the terminal).
    prompt_focused: bool,
    git: GitState,
    viewer: Viewer,
    /// A close that waits for confirmation, with the programs it would end.
    pending_close: Option<(CloseTarget, Vec<String>)>,
    term_find: Option<TermFind>,
    /// A one-line problem report (e.g. a link that could not be opened).
    notice: Option<String>,
    /// The session file text last written, to save only on changes.
    saved_session: Option<String>,
    /// The last session save error shown (shown once, not at every attempt).
    session_save_error: Option<String>,
    monitor: Monitor,
    mission: Mission,
    git_dash: GitDash,
    worktree_form: Option<WorktreeForm>,
    /// Worktree removal waiting for confirmation: (repository root, folder, branch).
    pending_worktree_remove: Option<(PathBuf, PathBuf, Option<String>)>,
    shortcuts_open: bool,
    /// The side panel shown (at most one).
    side: Option<SidePanel>,
    compare: Compare,
    /// Text to type into a new terminal once its shell shows a prompt (never Enter).
    pending_inserts: Vec<PendingInsert>,
}

struct PendingInsert {
    id: TermId,
    text: String,
    since: std::time::Instant,
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
            window_focused: true,
            explorer: Tree::new(launch_dir()),
            explorer_focused: false,
            explorer_goto: None,
            explorer_scroll: (0.0, 0.0),
            explorer_checking: false,
            explorer_follow: true,
            explorer_synced: None,
            revealed_tab: (0, 0),
            drop_hover: false,
            settings: Settings::default(),
            settings_path: forgeterm_settings::default_path(),
            settings_mtime: None,
            settings_problems: Vec::new(),
            start_dir: launch_dir(),
            agents: AgentKind::ALL
                .iter()
                .map(|k| (*k, AgentState::Detecting))
                .collect(),
            ai_context: None,
            ai_context_for: None,
            prompt_studio: None,
            prompt_docked: false,
            prompt_focused: false,
            git: GitState::default(),
            viewer: Viewer::default(),
            pending_close: None,
            term_find: None,
            notice: None,
            saved_session: None,
            session_save_error: None,
            monitor: Monitor::default(),
            mission: Mission::default(),
            git_dash: GitDash::default(),
            worktree_form: None,
            pending_worktree_remove: None,
            shortcuts_open: false,
            side: None,
            compare: Compare::default(),
            pending_inserts: Vec::new(),
        };
        app.menus = menu::menus(app.shells.len());
        app.load_settings(true);
        let terminal = app.restore_or_new_tab();
        let root = app.explorer.root().to_path_buf();
        (
            app,
            Task::batch([terminal, load_dir(root), detect_agents()]),
        )
    }

    // ---- terminal lifecycle -------------------------------------------------------

    /// Creates a terminal in `cwd` (default: the directory ForgeTerm started in).
    /// Its shell starts on the first layout, at the pane's real size.
    fn spawn_terminal(&mut self, shell: ShellSpec, cwd: Option<PathBuf>) -> TermId {
        self.spawn_terminal_for(shell, cwd, None)
    }

    /// Like [`Self::spawn_terminal`]; agents run without shell integration.
    fn spawn_terminal_for(
        &mut self,
        shell: ShellSpec,
        cwd: Option<PathBuf>,
        agent: Option<AgentKind>,
    ) -> TermId {
        let id = self.next_id;
        self.next_id += 1;
        let cwd = Some(cwd.unwrap_or_else(|| self.start_dir.clone()));
        let options = SessionOptions {
            scrollback: self.settings.terminal.scrollback,
            shell_integration: agent.is_none() && self.settings.terminal.shell_integration,
        };
        let mut pane = TerminalPane::new(id, shell, cwd, options, &self.palette);
        pane.agent = agent;
        self.terminals.insert(id, pane);
        id
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
        // Only a local folder is checked: `is_dir` on a network path can block the
        // window for as long as the share takes to answer.
        self.start_dir = match &new.terminal.start_directory {
            Some(dir)
                if !dir.to_str().is_some_and(forgeterm_terminal::is_local_path)
                    || !forgeterm_fs::resolves_locally(dir) =>
            {
                problems.push(format!(
                    "terminal.start_directory = \"{}\" is not a local folder; using {}",
                    dir.display(),
                    launch_dir().display()
                ));
                launch_dir()
            }
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
            // `explorer.visible = true` opens it (closing any other side panel);
            // false closes it only if it is the one shown.
            if e_new.visible {
                self.side = Some(SidePanel::Explorer);
            } else if self.panel_open(SidePanel::Explorer) {
                self.side = None;
            }
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
        // Shortcuts: defaults plus [keys]; rebuilt on every load (cheap).
        let (keymap, key_problems) = Keymap::with_overrides(&new.keys);
        self.keymap = keymap;
        problems.extend(key_problems);

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
        let id = self.spawn_terminal(shell, cwd);
        self.workspace.push_tab(id);
        Task::none()
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
        let id = self.spawn_terminal(shell, cwd);
        let split = self
            .workspace
            .active_tab_mut()
            .and_then(|tab| tab.split(axis, id));
        if split.is_some() {
            self.refresh_visible();
            Task::none()
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
        self.note_ended(id);
        self.terminals.remove(&id);
        if self.pending_paste.as_ref().is_some_and(|(t, _)| *t == id) {
            self.pending_paste = None;
        }
        self.after_layout_change()
    }

    fn close_tab(&mut self, index: usize) -> Task<Message> {
        for id in self.workspace.remove_tab(index) {
            self.note_ended(id);
            self.terminals.remove(&id);
        }
        self.after_layout_change()
    }

    fn after_layout_change(&mut self) -> Task<Message> {
        if self.workspace.tabs.is_empty() {
            // Last tab closed: the next start begins fresh, not with that tab.
            self.save_session();
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
        if !self.panel_open(SidePanel::Explorer) || !self.explorer_follow {
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

    /// The docked studio takes or returns the keyboard; the focused terminal redraws
    /// its cursor (hollow while the keyboard is elsewhere).
    fn set_prompt_focused(&mut self, focused: bool) {
        if self.prompt_focused != focused {
            self.prompt_focused = focused;
            if let Some(t) = self
                .workspace
                .focused()
                .and_then(|id| self.terminals.get(&id))
            {
                t.cache.clear();
            }
        }
    }

    /// The explorer takes the keyboard (the viewer and docked studio let go).
    fn focus_explorer(&mut self) {
        self.explorer_focused = true;
        self.viewer.focused = false;
        self.set_prompt_focused(false);
    }

    /// Scrolls the explorer list so the selected row is visible.
    fn reveal_explorer_selection(&self) -> Task<Message> {
        const ROW: f32 = 22.0;
        let Some(index) = self.explorer.selected_row() else {
            return Task::none();
        };
        let (offset, height) = self.explorer_scroll;
        let top = index as f32 * ROW;
        let y = if top < offset {
            top
        } else if height > ROW && top + ROW > offset + height {
            top + ROW - height
        } else {
            return Task::none();
        };
        iced::widget::operation::scroll_to(
            EXPLORER_LIST_ID,
            iced::widget::operation::AbsoluteOffset {
                x: None,
                y: Some(y),
            },
        )
    }

    fn focus_terminal(&mut self, id: TermId) {
        self.viewer.focused = false;
        self.explorer_focused = false;
        self.set_prompt_focused(false);
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

    // ---- side panels ----------------------------------------------------------------

    fn panel_open(&self, panel: SidePanel) -> bool {
        self.side == Some(panel)
    }

    /// Shows `panel` (replacing any other) or hides it; returns whether it is shown.
    fn toggle_panel(&mut self, panel: SidePanel) -> bool {
        self.side = (!self.panel_open(panel)).then_some(panel);
        self.side.is_some()
    }

    // ---- iced entry points --------------------------------------------------------

    pub fn title(&self) -> String {
        match self.focused() {
            Some(t) => format!("{} — ForgeTerm", t.display_title()),
            None => "ForgeTerm".into(),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        // A cheap stat every 2 s picks up saved edits to settings.toml; git status
        // every 4 s (a process) keeps the status bar current after commands typed
        // in the terminal. Both only while the window has focus: nobody sees them
        // in the background, and regaining focus catches up at once.
        let (settings, git) = if self.window_focused {
            (
                iced::time::every(Duration::from_secs(2)).map(|_| Message::SettingsTick),
                iced::time::every(Duration::from_secs(4)).map(|_| Message::GitTick),
            )
        } else {
            (Subscription::none(), Subscription::none())
        };
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
            Event::Window(window::Event::Focused) => Some(Message::WindowFocus(true)),
            Event::Window(window::Event::Unfocused) => Some(Message::WindowFocus(false)),
            _ => None,
        });
        let close = iced::window::close_requests().map(|_| Message::WindowCloseRequested);
        // Test hook for platforms where the window cannot be captured from outside
        // (WSLg): with FORGETERM_SCREENSHOT=<file.png> the rendered frame is saved
        // every 4 s. Off unless the variable is set.
        let screenshot = if std::env::var_os("FORGETERM_SCREENSHOT").is_some() {
            iced::time::every(Duration::from_secs(4)).map(|_| Message::DebugScreenshot)
        } else {
            Subscription::none()
        };
        let explorer = if self.panel_open(SidePanel::Explorer) {
            iced::time::every(Duration::from_secs(2)).map(|_| Message::ExplorerTick)
        } else {
            Subscription::none()
        };
        let mission = if self.mission.visible {
            iced::time::every(Duration::from_secs(1)).map(|_| Message::MissionTick)
        } else {
            Subscription::none()
        };
        let compare = if self.compare.visible || !self.pending_inserts.is_empty() {
            iced::time::every(Duration::from_millis(500)).map(|_| Message::CompareTick)
        } else {
            Subscription::none()
        };
        Subscription::batch([
            events, settings, git, close, screenshot, explorer, mission, compare,
        ])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.handle(message);
        self.acknowledge_seen();
        let follow_ups: Vec<Task<Message>> = [
            self.sync_explorer(),
            self.reveal_active_tab(),
            self.sync_ai_context(),
            self.sync_git(),
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
        if !self.panel_open(SidePanel::Ai) {
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
        let here = self.focused_cwd().unwrap_or_else(|| self.start_dir.clone());
        let root = forgeterm_agents::project_root(&here).0;
        self.start_agent_in(kind, root, split)
    }

    /// Starts an agent in `root` (a project root or worktree), as a new tab or split.
    fn start_agent_in(&mut self, kind: AgentKind, root: PathBuf, split: bool) -> Task<Message> {
        let Some(agent) = self.found_agent(kind) else {
            return Task::none();
        };
        let launch = match forgeterm_agents::launch_command(&agent.path) {
            Ok(l) => l,
            Err(e) => {
                self.notice = Some(e);
                return Task::none();
            }
        };
        let shell = ShellSpec::new(kind.name(), launch.program, launch.args);
        if split {
            self.restore_zoom();
        }
        let id = self.spawn_terminal_for(shell, Some(root), Some(kind));
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
        Task::none()
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
                // Mission Control covers the panes: nothing there is visible or
                // "seen"; agent previews refresh once per second (MissionTick).
                let visible = !self.main_covered() && self.workspace.is_visible(id);
                let seen = visible && self.window_focused;
                let Some(terminal) = self.terminals.get_mut(&id) else {
                    return Task::none();
                };
                match terminal.on_session_event(generation, event, visible, seen, &self.palette) {
                    Outcome::None => {}
                    // The tab shows it; outside the window the taskbar button flashes.
                    Outcome::Attention if !self.window_focused => {
                        return iced::window::latest().and_then(|w| {
                            iced::window::request_user_attention(
                                w,
                                Some(iced::window::UserAttention::Informational),
                            )
                        });
                    }
                    Outcome::Attention => {}
                    Outcome::Clipboard(text) => return self.program_clipboard(id, text),
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
            // Through the command, so a page covering the terminals (Mission Control,
            // Git Control Center) closes too, also when the active tab is clicked.
            Message::SelectTab(index) => return self.run_command(CommandId::SelectTab(index)),
            Message::WindowCloseRequested => return self.request_close(CloseTarget::Quit),
            Message::CloseChecked(target, programs) => {
                if programs.is_empty() {
                    return self.perform_close(target);
                }
                self.pending_close = Some((target, programs));
            }
            Message::ConfirmClose => {
                if let Some((target, _)) = self.pending_close.take() {
                    return self.perform_close(target);
                }
            }
            Message::CancelClose => self.pending_close = None,
            Message::TermFindQuery(query) => {
                // Search as you type, after a short pause (a long scrollback takes
                // milliseconds per search).
                let Some(f) = &mut self.term_find else {
                    return Task::none();
                };
                f.query = query;
                f.seq += 1;
                f.searched = false;
                f.outcome = None;
                let seq = f.seq;
                return background(
                    move || {
                        std::thread::sleep(TERM_FIND_DEBOUNCE);
                        seq
                    },
                    Message::TermFindDue,
                );
            }
            Message::TermFindDue(seq) => {
                if self
                    .term_find
                    .as_ref()
                    .is_some_and(|f| f.seq == seq && !f.searched)
                {
                    self.term_find_first();
                }
            }
            Message::TermFindStep(older) => {
                let Some(f) = &self.term_find else {
                    return Task::none();
                };
                if !f.searched {
                    // Enter before the pause ended: search now.
                    self.term_find_first();
                } else {
                    let query = f.query.clone();
                    let outcome = self.term_search(&query, older);
                    if let Some(f) = &mut self.term_find {
                        f.outcome = outcome;
                    }
                }
            }
            Message::TermFindClose => self.term_find = None,
            Message::DismissNotice => self.notice = None,
            Message::MonitorSampled(sample) => {
                self.record_mission_cpu(&sample);
                self.monitor.record(*sample);
            }
            Message::MissionTick => {
                self.mission.ticks = self.mission.ticks.wrapping_add(1);
                for t in self.terminals.values_mut().filter(|t| t.agent.is_some()) {
                    t.refresh_if_dirty(&self.palette);
                }
                if self.mission.ticks.is_multiple_of(5) {
                    return self.read_mission_heads();
                }
            }
            Message::MissionHeads(heads) => {
                self.mission.heads_busy = false;
                self.mission.heads.extend(heads);
            }
            Message::DebugScreenshot => {
                return iced::window::latest()
                    .and_then(iced::window::screenshot)
                    .map(Message::DebugScreenshotTaken);
            }
            Message::DebugScreenshotTaken(shot) => {
                if let Some(path) = std::env::var_os("FORGETERM_SCREENSHOT") {
                    save_screenshot(&shot, Path::new(&path));
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
            Message::FileRefResolved {
                wanted,
                line,
                found,
            } => match found {
                Some(path) => {
                    self.viewer.pending_line =
                        line.map(|l| (path.clone(), (l as usize).saturating_sub(1)));
                    return self.viewer_msg(ViewerMsg::Open(path));
                }
                None => {
                    self.notice = Some(format!(
                        "Not opened: {wanted} is not a file in this terminal's folder or its project."
                    ));
                }
            },
            Message::WindowFocus(focused) => {
                let back = focused && !self.window_focused;
                self.window_focused = focused;
                // The idle timers pause in the background: catch up once.
                if back {
                    return Task::batch([
                        Task::done(Message::SettingsTick),
                        self.refresh_git(false),
                    ]);
                }
            }
            Message::ShortcutsClose => self.shortcuts_open = false,
            Message::ExplorerListed(path, result) => self.explorer.set_listing(path, result),
            Message::ExplorerSelect(path) => {
                self.explorer.select(Some(path));
                self.focus_explorer();
            }
            Message::ExplorerToggle(path) => {
                self.explorer.select(Some(path.clone()));
                self.focus_explorer();
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
            Message::ExplorerTick => {
                if self.panel_open(SidePanel::Explorer) && !self.explorer_checking {
                    let shown = self.explorer.shown_dirs();
                    if !shown.is_empty() {
                        self.explorer_checking = true;
                        return background(
                            move || forgeterm_explorer::changed_dirs(&shown),
                            Message::ExplorerChanged,
                        );
                    }
                }
            }
            Message::ExplorerChanged(dirs) => {
                self.explorer_checking = false;
                return Task::batch(dirs.into_iter().map(load_dir));
            }
            Message::ExplorerScrolled(viewport) => {
                self.explorer_scroll = (viewport.absolute_offset().y, viewport.bounds().height);
            }
            Message::ExplorerGoToInput(text) => {
                if let Some(goto) = &mut self.explorer_goto {
                    *goto = (text, None);
                }
            }
            Message::ExplorerGoToSubmit => {
                let Some((text, _)) = &self.explorer_goto else {
                    return Task::none();
                };
                let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                    .map(PathBuf::from);
                let wanted = forgeterm_explorer::folder_from_input(
                    text,
                    self.explorer.root(),
                    home.as_deref(),
                );
                return background(
                    move || {
                        let dir = wanted?;
                        // Same rule as folders reported by shells: never a network path.
                        if !dir.to_str().is_some_and(forgeterm_terminal::is_local_path)
                            || !forgeterm_fs::resolves_locally(&dir)
                        {
                            return Err(format!("{} is not a local folder.", dir.display()));
                        }
                        if dir.is_dir() {
                            Ok(dir)
                        } else {
                            Err(format!("{} is not a folder.", dir.display()))
                        }
                    },
                    Message::ExplorerGoToResult,
                );
            }
            Message::ExplorerGoToResult(result) => match result {
                Ok(dir) => {
                    self.explorer_goto = None;
                    self.side = Some(SidePanel::Explorer);
                    self.focus_explorer();
                    return Task::batch([load_dir(self.explorer.set_root(dir)), unfocus_widgets()]);
                }
                Err(e) => {
                    if let Some(goto) = &mut self.explorer_goto {
                        goto.1 = Some(e);
                    }
                }
            },
            Message::FileHovered => self.drop_hover = true,
            Message::FilesHoveredLeft => self.drop_hover = false,
            Message::SettingsTick => {
                if self.settings_changed_on_disk() {
                    self.load_settings(false);
                }
                self.save_session();
                let sample = self.sample_monitor();
                // Files open in the viewer that changed on disk are reloaded.
                let changed: Vec<PathBuf> = self
                    .viewer
                    .tabs
                    .iter_mut()
                    .filter(|t| !t.reloading)
                    .filter(|t| {
                        let now = forgeterm_viewer::modified(&t.doc.path);
                        now.is_some() && now != t.doc.modified
                    })
                    .map(|t| {
                        t.reloading = true;
                        t.doc.path.clone()
                    })
                    .collect();
                return Task::batch(changed.into_iter().map(load_document).chain(sample));
            }
            Message::DismissSettingsProblems => self.settings_problems.clear(),
            Message::WorkerPanicked => {
                // Which task it was is unknown here: free every "running" flag, so
                // nothing stays stuck (at worst one job runs twice), and say so.
                tracing::error!("a background task panicked");
                self.git.refreshing = None;
                self.git.action_running = false;
                self.monitor.busy = false;
                self.explorer_checking = false;
                self.git_dash.busy = false;
                self.mission.heads_busy = false;
                self.notice =
                    Some("A background task failed unexpectedly; it will be retried.".to_owned());
            }
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
            Message::PromptsLoaded(library) => {
                if let Some(studio) = &mut self.prompt_studio {
                    studio.set_library(library);
                }
            }
            Message::PromptSelect(index) => {
                if let Some(studio) = &mut self.prompt_studio {
                    studio.select(index);
                }
            }
            Message::PromptEdit(name, action) => {
                if let Some(studio) = &mut self.prompt_studio {
                    studio.edit(name, action);
                    // Clicking or typing in a field: the keyboard belongs to the studio.
                    self.set_prompt_focused(self.prompt_docked);
                }
            }
            Message::PromptClose => {
                self.prompt_studio = None;
                self.set_prompt_focused(false);
            }
            Message::GitTick => return self.refresh_git(false),
            Message::GitRefreshed(snapshot) => {
                if self.git.refreshing.as_ref() == Some(&snapshot.dir) {
                    self.git.refreshing = None;
                }
                // A result for a folder the terminal has left is dropped; the
                // follow-up sync starts a refresh for the new one.
                if snapshot.dir == self.git_dir() {
                    self.git.apply(*snapshot);
                    return self.load_git_dash(false);
                }
            }
            Message::WorktreeFormAgent(kind) => {
                if let Some(f) = &mut self.worktree_form {
                    if worktree_form::is_suggestion(&f.branch) {
                        f.branch = worktree_form::suggest_branch(kind, &f.existing);
                    }
                    f.kind = kind;
                }
            }
            Message::WorktreeFormBranches(names) => {
                if let Some(f) = &mut self.worktree_form
                    && f.set_existing(names)
                {
                    return iced::widget::operation::select_all(worktree_form::BRANCH_INPUT_ID);
                }
            }
            Message::WorktreeFormBranch(branch) => {
                if let Some(f) = &mut self.worktree_form {
                    f.branch = branch;
                    f.error = None;
                }
            }
            Message::WorktreeFormSubmit => {
                let Some(f) = self.worktree_form.as_mut().filter(|f| !f.busy) else {
                    return Task::none();
                };
                let Some(path) = forgeterm_git::default_worktree_path(&f.root, &f.branch) else {
                    f.error = Some("Type a branch name.".to_owned());
                    return Task::none();
                };
                f.busy = true;
                f.error = None;
                let (root, branch) = (f.root.clone(), f.branch.trim().to_owned());
                return background(
                    move || forgeterm_git::add_worktree(&root, &path, &branch).map(|()| path),
                    Message::WorktreeCreated,
                );
            }
            Message::WorktreeFormCancel => {
                if self.worktree_form.as_ref().is_some_and(|f| !f.busy) {
                    self.worktree_form = None;
                }
            }
            Message::WorktreeCreated(Ok(path)) => {
                let Some(form) = self.worktree_form.take() else {
                    return Task::none();
                };
                self.mission.visible = false;
                self.git_dash.visible = false;
                self.compare.visible = false;
                self.refresh_visible();
                if self.found_agent(form.kind).is_none() {
                    self.notice = Some(format!(
                        "Worktree created at {}, but {} was not found: no agent started.",
                        path.display(),
                        form.kind.name()
                    ));
                }
                let start = self.start_agent_in(form.kind, path, false);
                return Task::batch([start, self.refresh_git(true)]);
            }
            Message::WorktreeCreated(Err(e)) => {
                if let Some(f) = &mut self.worktree_form {
                    f.busy = false;
                    f.error = Some(e);
                }
            }
            Message::WorktreeRemoveConfirm => {
                if let Some((root, path, _)) = self.pending_worktree_remove.take() {
                    return background(
                        move || forgeterm_git::remove_worktree(&root, &path),
                        Message::WorktreeRemoved,
                    );
                }
            }
            Message::WorktreeRemoveCancel => self.pending_worktree_remove = None,
            Message::CompareLoaded(data) => {
                if data.seq != self.compare.seq {
                    return Task::none();
                }
                self.compare.busy = false;
                if self
                    .compare
                    .data
                    .as_ref()
                    .is_some_and(|d| d.root != data.root)
                {
                    self.compare.test_command_edited = false;
                }
                if !self.compare.test_command_edited {
                    self.compare.test_command = data.test_command.unwrap_or_default().to_owned();
                }
                if let Some([a, b]) = &data.sides {
                    self.compare.a = Some(a.worktree.path.clone());
                    self.compare.b = Some(b.worktree.path.clone());
                }
                let still = data.sides.as_ref().is_some_and(|[a, b]| {
                    let has = |s: &compare_view::Side, p: &str| {
                        s.files
                            .as_ref()
                            .is_ok_and(|f| f.iter().any(|f| f.path == p))
                    };
                    self.compare
                        .selected
                        .as_deref()
                        .is_some_and(|p| has(a, p) || has(b, p))
                });
                self.compare.data = Some(*data);
                if let Some(path) = self.compare.selected.clone() {
                    if still {
                        // Its contents may have changed: read it again (the old rows
                        // stay until then).
                        return self.load_compare_diff(path, false);
                    }
                    self.compare.selected = None;
                    self.compare.diff = None;
                }
            }
            Message::ComparePickA(c) => {
                if self.compare.b.as_ref() == Some(&c.path) {
                    self.compare.b = self.compare.a.take();
                }
                self.compare.a = Some(c.path);
                return self.compare_sides_changed();
            }
            Message::ComparePickB(c) => {
                if self.compare.a.as_ref() == Some(&c.path) {
                    self.compare.a = self.compare.b.take();
                }
                self.compare.b = Some(c.path);
                return self.compare_sides_changed();
            }
            Message::CompareSelect(path) => return self.load_compare_diff(path, true),
            Message::CompareDiffLoaded(pair, path, result) => {
                // Only for the pair shown now (sides may have been picked or swapped
                // meanwhile).
                if self.compare.paths().as_ref() == Some(&pair)
                    && let Some(d) = &mut self.compare.diff
                    && d.path == path
                {
                    d.result = Some(result);
                }
            }
            Message::CompareTestCommand(command) => {
                self.compare.test_command = command;
                self.compare.test_command_edited = true;
            }
            Message::ComparePromptEdit(action) => self.compare.prompt.perform(action),
            Message::CompareTick => {
                self.process_pending_inserts();
                if self.compare.visible && !self.compare.busy {
                    let root = self.git.root().cloned();
                    let stale = self.compare.data.as_ref().is_none_or(|d| {
                        Some(&d.root) != root.as_ref()
                            || d.loaded_at.elapsed() > Duration::from_secs(10)
                    });
                    if stale {
                        return self.load_compare();
                    }
                }
            }
            Message::WorktreeRemoved(result) => {
                match result {
                    Err(e) => self.notice = Some(format!("{REMOVE_FAILED}{e}")),
                    // An earlier refusal no longer applies.
                    Ok(()) => {
                        if self
                            .notice
                            .as_deref()
                            .is_some_and(|n| n.starts_with(REMOVE_FAILED))
                        {
                            self.notice = None;
                        }
                    }
                }
                return self.load_git_dash(true);
            }
            Message::GitDashLoaded(data) => {
                self.git_dash.busy = false;
                self.git_dash.data = Some(*data);
                if std::mem::take(&mut self.git_dash.reload_pending) {
                    return self.load_git_dash(true);
                }
            }
            Message::GitStage(paths) => return self.git_action(GitAction::Stage(paths)),
            Message::GitUnstage(paths) => return self.git_action(GitAction::Unstage(paths)),
            Message::GitCommitEdit(action) => self.git.commit_message.perform(action),
            Message::GitActionDone(result) => {
                self.git.action_running = false;
                if result.is_ok() && self.git.last_action_was_commit {
                    self.git.commit_message = iced::widget::text_editor::Content::new();
                }
                if result.is_ok() && self.git.last_action_was_create {
                    self.git.new_branch.clear();
                }
                self.git.last_action = Some(result);
                return self.refresh_git(true);
            }
            Message::GitShowDiff(path, kind) => {
                let Some(root) = self.git.root().cloned() else {
                    return Task::none();
                };
                self.git.diff = Some(DiffView {
                    path: path.clone(),
                    kind,
                    result: None,
                });
                return background(
                    {
                        let path = path.clone();
                        move || forgeterm_git::diff(&root, &path, kind)
                    },
                    move |result| Message::GitDiffLoaded(path.clone(), kind, result),
                );
            }
            Message::GitDiffLoaded(path, kind, result) => {
                if let Some(d) = &mut self.git.diff
                    && d.path == path
                    && d.kind == kind
                {
                    d.result = Some(result);
                }
            }
            Message::GitDiffClose => self.git.diff = None,
            Message::GitHistoryLoaded(data) => {
                if self.git.root() == Some(&data.root)
                    && let Some(h) = &mut self.git.history
                {
                    h.entries = Some(data.entries);
                    h.activity = Some(data.activity);
                }
            }
            Message::GitSelectCommit(hash) => {
                let (Some(root), Some(h)) = (self.git.root().cloned(), &mut self.git.history)
                else {
                    return Task::none();
                };
                h.selected = Some(hash.clone());
                h.detail = None;
                return background(
                    {
                        let hash = hash.clone();
                        move || forgeterm_git::show_commit(&root, &hash)
                    },
                    move |result| Message::GitCommitLoaded(hash.clone(), result),
                );
            }
            Message::GitCommitLoaded(hash, result) => {
                if let Some(h) = &mut self.git.history
                    && h.selected.as_deref() == Some(hash.as_str())
                {
                    h.detail = Some(result);
                }
            }
            Message::GitHistoryClose => self.git.history = None,
            Message::GitSwitchAsk(name) => self.git.pending_switch = Some(name),
            Message::GitSwitchCancel => self.git.pending_switch = None,
            Message::GitSwitchConfirm => {
                if let Some(name) = self.git.pending_switch.take() {
                    return self.git_action(GitAction::Switch(name));
                }
            }
            Message::GitTrack(name) => return self.git_action(GitAction::Track(name)),
            Message::GitNewBranchName(name) => self.git.new_branch = name,
            Message::GitCreateBranch => {
                let name = self.git.new_branch.trim().to_owned();
                if !name.is_empty() {
                    return self.git_action(GitAction::Create(name));
                }
            }
            Message::GitOpenInViewer(rel) => {
                if let Some(root) = self.git.root().cloned() {
                    self.git.diff = None;
                    return self.viewer_msg(ViewerMsg::Open(root.join(rel)));
                }
            }
            Message::Viewer(msg) => return self.viewer_msg(msg),
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
            TermMsg::GridResized(cols, rows) => return t.resize(cols, rows, metrics, palette),
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
            TermMsg::Paste => return self.run_command(CommandId::Paste),
            TermMsg::OpenUrl(url) => {
                if let Err(e) = open_url(&url) {
                    self.notice = Some(e);
                }
            }
            TermMsg::OpenFile { path, line } => {
                return resolve_file_ref(path, line, t.directory().map(Path::to_path_buf));
            }
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
        // A close confirmation answers Enter / Esc and swallows everything else.
        if self.pending_close.is_some() {
            return match key {
                keyboard::Key::Named(Named::Enter) => self.update(Message::ConfirmClose),
                keyboard::Key::Named(Named::Escape) => self.update(Message::CancelClose),
                _ => Task::none(),
            };
        }
        // While a paste confirmation is open, the keyboard answers it and nothing else.
        if self.pending_paste.is_some() {
            return match key {
                keyboard::Key::Named(Named::Enter) => self.update(Message::ConfirmPaste),
                keyboard::Key::Named(Named::Escape) => self.update(Message::CancelPaste),
                _ => Task::none(),
            };
        }

        // The worktree dialog: Esc cancels; Enter submits through the branch field
        // (or here when the field does not have the keyboard); nothing else leaks.
        if self.worktree_form.is_some() {
            return match key {
                keyboard::Key::Named(Named::Escape) => self.update(Message::WorktreeFormCancel),
                keyboard::Key::Named(Named::Enter) if !captured => {
                    self.update(Message::WorktreeFormSubmit)
                }
                _ => Task::none(),
            };
        }
        if self.pending_worktree_remove.is_some() {
            return match key {
                keyboard::Key::Named(Named::Enter) => self.update(Message::WorktreeRemoveConfirm),
                keyboard::Key::Named(Named::Escape) => self.update(Message::WorktreeRemoveCancel),
                _ => Task::none(),
            };
        }
        // Modal overlays own the keyboard while open.
        if self.shortcuts_open {
            if matches!(key, keyboard::Key::Named(Named::Escape | Named::Enter)) {
                self.shortcuts_open = false;
            }
            return Task::none();
        }
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
        if self.prompt_studio.is_some() && !self.prompt_docked {
            return self.prompt_studio_key(&key, mods, captured);
        }
        if self.git.diff.is_some() {
            if matches!(key, keyboard::Key::Named(Named::Escape)) {
                self.git.diff = None;
            }
            return Task::none();
        }
        if self.git.history.is_some() {
            if matches!(key, keyboard::Key::Named(Named::Escape)) {
                self.git.history = None;
            }
            return Task::none();
        }
        // Terminal search bar: Esc closes, F3 / Shift+F3 step, also from its input.
        if self.term_find.is_some() && !self.viewer.focused {
            match key {
                keyboard::Key::Named(Named::Escape) => {
                    self.term_find = None;
                    return Task::none();
                }
                keyboard::Key::Named(Named::F3) => {
                    return self.update(Message::TermFindStep(!mods.shift()));
                }
                // The match is the terminal selection: Ctrl+C copies it, even while
                // the search field (which would swallow the key) has the focus.
                keyboard::Key::Character(c)
                    if mods.control()
                        && !mods.shift()
                        && !mods.alt()
                        && c.eq_ignore_ascii_case("c")
                        && self
                            .focused()
                            .and_then(TerminalPane::session)
                            .is_some_and(|s| s.selection_text().is_some()) =>
                {
                    return self.copy_selection();
                }
                _ => {}
            }
        }
        // The viewer's find bar closes with Esc, also while its input has focus.
        if self.viewer.visible
            && self.viewer.find_open
            && matches!(key, keyboard::Key::Named(Named::Escape))
            && (self.viewer.focused || captured)
        {
            return self.viewer_msg(ViewerMsg::CloseFind);
        }
        // Docked studio with the keyboard: Esc gives it back to the terminal (the field
        // also lets go of it), Tab moves between fields.
        let studio_keys = self.prompt_docked && self.prompt_focused && self.prompt_studio.is_some();
        if studio_keys {
            if matches!(key, keyboard::Key::Named(Named::Escape)) {
                self.set_prompt_focused(false);
                return unfocus_widgets();
            }
            if !captured && matches!(key, keyboard::Key::Named(Named::Tab)) {
                return if mods.shift() {
                    iced::widget::operation::focus_previous()
                } else {
                    iced::widget::operation::focus_next()
                };
            }
        }
        // The "Go to Folder" field closes with Esc, also while it has the keyboard.
        if self.explorer_goto.is_some() && matches!(key, keyboard::Key::Named(Named::Escape)) {
            self.explorer_goto = None;
            return unfocus_widgets();
        }
        // Keys handled by a widget never reach the terminal.
        if captured {
            return Task::none();
        }
        // Keys the studio's fields do not use (F-keys, Ctrl+letter, …) never leak to
        // the terminal either; app shortcuts still work.
        if studio_keys {
            return self.app_shortcut(&key, mods).unwrap_or_else(Task::none);
        }
        // The focused viewer takes navigation keys; app shortcuts still work, the
        // terminal gets nothing.
        if self.viewer.visible && self.viewer.focused {
            if let Some(msg) = viewer_key(&key, mods, &self.viewer) {
                return self.viewer_msg(msg);
            }
            return self.app_shortcut(&key, mods).unwrap_or_else(Task::none);
        }

        // The focused explorer takes navigation keys (Esc gives the keyboard back
        // to the terminal); app shortcuts still work; other keys go to the terminal.
        if self.panel_open(SidePanel::Explorer) && self.explorer_focused {
            if matches!(key, keyboard::Key::Named(Named::Escape)) {
                self.explorer_focused = false;
                return Task::none();
            }
            if let Some(nav) = explorer_key(&key, mods) {
                let page = ((self.explorer_scroll.1 / 22.0) as usize).max(1);
                let task = match self.explorer.navigate(nav, page) {
                    forgeterm_explorer::NavOutcome::None => Task::none(),
                    forgeterm_explorer::NavOutcome::Load(dir) => load_dir(dir),
                    // Like an editor: the opened file gets the keyboard (Esc there
                    // returns it to the terminal).
                    forgeterm_explorer::NavOutcome::Open(file) => {
                        self.viewer_msg(ViewerMsg::Open(file))
                    }
                };
                return Task::batch([task, self.reveal_explorer_selection()]);
            }
            if let Some(task) = self.app_shortcut(&key, mods) {
                return task;
            }
            // A modifier on its own (Shift before a capital, Ctrl before a shortcut,
            // Alt) is not typing yet: the explorer keeps the keyboard.
            if matches!(
                key,
                keyboard::Key::Named(
                    Named::Shift | Named::Control | Named::Alt | Named::Super | Named::AltGraph
                )
            ) {
                return Task::none();
            }
            // Any other key is typing meant for the terminal: it gets the keyboard
            // back, including this key, so nothing typed is lost.
            self.explorer_focused = false;
        }

        if let Some(task) = self.app_shortcut(&key, mods) {
            return task;
        }
        if self.mission.visible {
            return self.mission_key(&key, mods);
        }
        // The Git Control Center has no keyboard actions of its own besides Esc;
        // nothing reaches the terminals hidden behind it.
        if self.git_dash.visible {
            if matches!(key, keyboard::Key::Named(Named::Escape)) {
                return self.run_command(CommandId::ToggleGitDashboard);
            }
            return Task::none();
        }
        // Compare has text fields; Esc closes it and nothing reaches the terminals.
        if self.compare.visible {
            if matches!(key, keyboard::Key::Named(Named::Escape)) {
                return self.run_command(CommandId::ToggleCompare);
            }
            return Task::none();
        }

        let Some(focused) = self.workspace.focused() else {
            return Task::none();
        };
        let Some(t) = self.terminals.get_mut(&focused) else {
            return Task::none();
        };

        // A pane whose shell ended: Enter starts it again.
        if !t.is_running() {
            // Not started yet (first layout pending): nothing to restart.
            if matches!(key, keyboard::Key::Named(Named::Enter)) && !t.is_awaiting_size() {
                return t.start(self.metrics, &self.palette);
            }
            return Task::none();
        }

        if let Some((tkey, tmods)) = keys::translate(&key, text.as_deref(), mods)
            && let Some(bytes) = forgeterm_terminal::encode_key_kitty(
                &tkey,
                tmods,
                t.modes.app_cursor,
                t.modes.keyboard,
            )
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

    /// The studio owns the keyboard: its editors take typing (including Ctrl+C/V/X),
    /// Tab moves between fields, Esc closes. Nothing reaches the terminal.
    fn prompt_studio_key(
        &mut self,
        key: &keyboard::Key,
        mods: keyboard::Modifiers,
        captured: bool,
    ) -> Task<Message> {
        let toggle = keys::binding(key, mods)
            .and_then(|b| self.keymap.lookup(b))
            .is_some_and(|id| id == CommandId::OpenPromptStudio);
        if matches!(key, keyboard::Key::Named(Named::Escape)) || (toggle && !captured) {
            self.prompt_studio = None;
            return Task::none();
        }
        if !captured && matches!(key, keyboard::Key::Named(Named::Tab)) {
            return if mods.shift() {
                iced::widget::operation::focus_previous()
            } else {
                iced::widget::operation::focus_next()
            };
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
        // A shortcut follows the same rule as the menu and the palette: a command
        // that does not apply now does nothing. The key is still ForgeTerm's, so
        // it does not fall through to the program either.
        if !self.is_applicable(id) {
            return Some(Task::none());
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
        if self.main_covered() && id.shows_terminals() {
            self.mission.visible = false;
            self.git_dash.visible = false;
            self.compare.visible = false;
            self.refresh_visible();
        }
        if id.is_recordable() {
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
            CommandId::CloseTab => {
                let ids = self
                    .workspace
                    .active_tab()
                    .map(|t| t.terminals().collect())
                    .unwrap_or_default();
                return self.request_close(CloseTarget::Tab(ids));
            }
            CommandId::ClosePane => {
                if let Some(id) = self.workspace.focused() {
                    return self.request_close(CloseTarget::Pane(id));
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
            CommandId::PreviousCommand | CommandId::NextCommand => {
                let palette = &self.palette;
                if let Some(focused) = self.workspace.focused()
                    && let Some(t) = self.terminals.get_mut(&focused)
                {
                    if let Some(s) = t.session() {
                        s.jump_to_prompt(id == CommandId::PreviousCommand);
                    }
                    t.refresh(palette);
                }
            }
            CommandId::CopyLastCommandOutput => {
                let output = self
                    .focused()
                    .and_then(TerminalPane::session)
                    .map(forgeterm_terminal::Session::last_command_output);
                match output {
                    Some(Ok(out)) => return clipboard::write(out.text),
                    Some(Err(e)) => self.notice = Some(format!("Nothing copied: {e}.")),
                    None => {}
                }
            }
            CommandId::InsertLastOutputIntoAgent => return self.insert_last_output_into_agent(),
            CommandId::Copy => return self.copy_selection(),
            CommandId::FindInTerminal => {
                if self.term_find.is_none() {
                    self.term_find = Some(TermFind::default());
                }
                return iced::widget::operation::focus(TERM_FIND_ID);
            }
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
            // Like VS Code: hidden → show with the keyboard, shown → take the
            // keyboard, already focused → hide (keyboard back to the terminal).
            CommandId::ToggleExplorer => {
                if self.panel_open(SidePanel::Explorer) && self.explorer_focused {
                    self.side = None;
                    self.explorer_focused = false;
                    self.explorer_goto = None;
                } else {
                    self.side = Some(SidePanel::Explorer);
                    self.focus_explorer();
                    return unfocus_widgets();
                }
            }
            CommandId::ExplorerGoToFolder => {
                self.side = Some(SidePanel::Explorer);
                self.focus_explorer();
                let current = self.explorer.root().to_string_lossy().into_owned();
                self.explorer_goto = Some((current, None));
                return Task::batch([
                    iced::widget::operation::focus(EXPLORER_GOTO_ID),
                    iced::widget::operation::select_all(EXPLORER_GOTO_ID),
                ]);
            }
            CommandId::ToggleAiPanel => {
                self.toggle_panel(SidePanel::Ai);
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
            CommandId::ToggleMissionControl => return self.toggle_mission(),
            CommandId::GoToTerminal(id) => self.go_to_terminal(id),
            CommandId::TerminalFolder(id) => {
                let Some(dir) = self
                    .terminals
                    .get(&id)
                    .and_then(TerminalPane::directory)
                    .filter(|d| d.to_str().is_some_and(forgeterm_terminal::is_local_path))
                    .map(Path::to_path_buf)
                else {
                    return Task::none();
                };
                self.side = Some(SidePanel::Explorer);
                return load_dir(self.explorer.set_root(dir));
            }
            CommandId::TerminalGit(id) => {
                self.go_to_terminal(id);
                self.side = Some(SidePanel::Git);
                return self.refresh_git(true);
            }
            CommandId::StopTerminal(id) => return self.request_close(CloseTarget::Pane(id)),
            CommandId::ToggleMonitor => {
                if self.toggle_panel(SidePanel::Monitor) {
                    return self.sample_monitor().unwrap_or_else(Task::none);
                }
            }
            CommandId::ToggleViewer => {
                self.viewer.visible = !self.viewer.visible;
                if !self.viewer.visible {
                    self.viewer.focused = false;
                }
            }
            CommandId::ViewSelectedFile => {
                if let Some(path) = self
                    .explorer
                    .selected()
                    .filter(|e| !e.is_dir)
                    .map(|e| e.path.clone())
                {
                    return self.viewer_msg(ViewerMsg::Open(path));
                }
            }
            CommandId::ToggleGitPanel => {
                if self.toggle_panel(SidePanel::Git) {
                    return self.refresh_git(true);
                }
            }
            CommandId::GitRefresh => {
                let compare = if self.compare.visible {
                    self.load_compare()
                } else {
                    Task::none()
                };
                return Task::batch([self.refresh_git(true), self.load_git_dash(true), compare]);
            }
            CommandId::CloseTabAt(index) => {
                let ids = self
                    .workspace
                    .tabs
                    .get(index)
                    .map(|t| t.terminals().collect())
                    .unwrap_or_default();
                return self.request_close(CloseTarget::Tab(ids));
            }
            CommandId::InsertPrompt => return self.insert_prompt(),
            CommandId::CopyPrompt => {
                let text = self
                    .prompt_studio
                    .as_ref()
                    .and_then(PromptStudio::rendered)
                    .map(|r| r.text);
                // The floating window closes; the docked panel stays.
                if !self.prompt_docked {
                    self.prompt_studio = None;
                }
                if let Some(text) = text {
                    return clipboard::write(text);
                }
            }
            CommandId::ReloadPrompts => {
                if self.prompt_studio.is_some() {
                    return load_prompts(self.prompts_dir());
                }
            }
            CommandId::GitCommit => {
                if self.git.can_commit() {
                    let message = self.git.commit_message.text();
                    return self.git_action(GitAction::Commit(message));
                }
            }
            CommandId::GitStageAll => {
                let paths = self.git.stage_all_paths();
                if !paths.is_empty() {
                    return self.git_action(GitAction::Stage(paths));
                }
            }
            CommandId::GitUnstageAll => {
                let paths = self.git.unstage_all_paths();
                if !paths.is_empty() {
                    return self.git_action(GitAction::Unstage(paths));
                }
            }
            CommandId::GitTrustRepository => {
                if let Some(Repo::Untrusted { root, .. }) = self.git.repo.as_ref() {
                    self.git.trusted.insert(root.clone());
                    return self.refresh_git(true);
                }
            }
            CommandId::GitToggleRemoteBranches => self.git.show_remotes = !self.git.show_remotes,
            CommandId::ToggleCompare => {
                self.compare.visible = !self.compare.visible;
                if !self.compare.visible {
                    self.refresh_visible();
                    return Task::none();
                }
                self.mission.visible = false;
                self.git_dash.visible = false;
                return Task::batch([self.refresh_git(true), self.load_compare()]);
            }
            CommandId::CompareSwap => {
                if let Some([a, b]) = self.compare.paths() {
                    self.compare.a = Some(b);
                    self.compare.b = Some(a);
                    // The same pair: the file stays selected, its rows swap sides
                    // once the load is in (`CompareLoaded` reads it again).
                    if let Some(d) = &mut self.compare.diff {
                        d.result = None;
                    }
                    return self.load_compare();
                }
            }
            CommandId::CompareOpenTests => return self.open_compare_tests(),
            CommandId::CompareInsertPrompt => self.insert_compare_prompt(),
            CommandId::ToggleGitDashboard => {
                self.git_dash.visible = !self.git_dash.visible;
                if !self.git_dash.visible {
                    self.refresh_visible();
                    return Task::none();
                }
                self.mission.visible = false;
                self.compare.visible = false;
                return Task::batch([self.refresh_git(true), self.load_git_dash(true)]);
            }
            CommandId::NewAgentWorktree => {
                let Some(Repo::Found { root, .. }) = self.git.repo.as_ref() else {
                    return Task::none();
                };
                let Some(kind) = AgentKind::ALL
                    .into_iter()
                    .find(|k| self.found_agent(*k).is_some())
                else {
                    return Task::none();
                };
                let existing = self.branch_names();
                let branch = worktree_form::suggest_branch(kind, &existing);
                self.worktree_form = Some(WorktreeForm {
                    root: root.clone(),
                    kind,
                    branch,
                    existing,
                    error: None,
                    busy: false,
                });
                // The panel's list can be missing or old: read it now.
                let root = root.clone();
                let names = background(
                    move || {
                        forgeterm_git::branches(&root)
                            .map(|l| l.into_iter().map(|b| b.name).collect())
                            .unwrap_or_default()
                    },
                    Message::WorktreeFormBranches,
                );
                return Task::batch([
                    iced::widget::operation::focus(worktree_form::BRANCH_INPUT_ID),
                    iced::widget::operation::select_all(worktree_form::BRANCH_INPUT_ID),
                    names,
                ]);
            }
            CommandId::RemoveWorktree(id) => {
                if let Some(target) = self.removable_worktree(id) {
                    self.pending_worktree_remove = Some(target);
                }
            }
            CommandId::WorktreeTerminal(i) => {
                if let Some(path) = self.worktree_path(i) {
                    return self.new_tab(None, Some(path));
                }
            }
            CommandId::GitShowHistory => {
                if let Some(root) = self.git.root().cloned() {
                    self.git.history = Some(HistoryView::default());
                    let key = root.clone();
                    return background(
                        move || HistoryData {
                            entries: forgeterm_git::history(&root, 300),
                            activity: forgeterm_git::activity(&root, 30),
                            root,
                        },
                        move |data| {
                            debug_assert_eq!(data.root, key);
                            Message::GitHistoryLoaded(Box::new(data))
                        },
                    );
                }
            }
            CommandId::OpenPromptStudio => {
                self.set_prompt_focused(false);
                if self.prompt_studio.take().is_none() {
                    return self.open_prompt_studio();
                }
            }
            CommandId::TogglePromptStudioDock => {
                self.prompt_docked = !self.prompt_docked;
                self.set_prompt_focused(false);
                if self.prompt_studio.is_none() {
                    return self.open_prompt_studio();
                }
            }
            CommandId::OpenPromptsFolder => {
                if let Some(dir) = self.prompts_dir()
                    && let Err(e) = open_prompts_folder(&dir)
                {
                    self.notice = Some(e);
                }
            }
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
            CommandId::Website => {
                if let Err(e) = open_url(crate::commands::WEBSITE) {
                    self.notice = Some(e);
                }
            }
            CommandId::KeyboardShortcuts => self.shortcuts_open = true,
            // Same as closing the window: sessions are dropped, shells are shut down.
            CommandId::Quit => return self.request_close(CloseTarget::Quit),
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
            CommandId::PreviousCommand
            | CommandId::NextCommand
            | CommandId::CopyLastCommandOutput => focused
                .and_then(TerminalPane::session)
                .is_some_and(forgeterm_terminal::Session::has_prompt_marks),
            CommandId::InsertLastOutputIntoAgent => {
                focused.is_some_and(|t| {
                    t.agent.is_none()
                        && t.session()
                            .is_some_and(forgeterm_terminal::Session::has_prompt_marks)
                }) && self.agent_target().is_some()
            }
            CommandId::Copy => focused
                .and_then(TerminalPane::session)
                .is_some_and(|s| s.selection_text().is_some()),
            CommandId::Paste
            | CommandId::FindInTerminal
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
            CommandId::OpenPromptsFolder => self.prompts_dir().is_some(),
            CommandId::GitShowHistory => self.git.root().is_some(),
            CommandId::ToggleGitDashboard | CommandId::ToggleCompare => true,
            CommandId::CloseTabAt(i) => i < self.workspace.tabs.len(),
            CommandId::InsertPrompt => {
                self.prompt_studio
                    .as_ref()
                    .and_then(PromptStudio::rendered)
                    .is_some_and(|r| r.missing.is_empty() && !r.text.is_empty())
                    && focused.is_some_and(TerminalPane::is_running)
            }
            CommandId::CopyPrompt => self
                .prompt_studio
                .as_ref()
                .and_then(PromptStudio::rendered)
                .is_some_and(|r| !r.text.is_empty()),
            CommandId::ReloadPrompts => self.prompt_studio.is_some(),
            CommandId::GitCommit => self.git.can_commit(),
            CommandId::GitStageAll => {
                !self.git.action_running && !self.git.stage_all_paths().is_empty()
            }
            CommandId::GitUnstageAll => {
                !self.git.action_running && !self.git.unstage_all_paths().is_empty()
            }
            CommandId::GitTrustRepository => {
                matches!(self.git.repo, Some(Repo::Untrusted { .. }))
            }
            CommandId::GitToggleRemoteBranches => self.git.root().is_some(),
            CommandId::CompareSwap => self.compare.visible && self.compare.paths().is_some(),
            CommandId::CompareOpenTests => {
                self.compare.paths().is_some() && !self.compare.test_command.trim().is_empty()
            }
            CommandId::CompareInsertPrompt => {
                self.compare.paths().is_some()
                    && !self.compare.prompt.text().trim().is_empty()
                    && self.compare_agents().iter().any(|l| !l.is_empty())
            }
            CommandId::NewAgentWorktree => {
                matches!(self.git.repo, Some(Repo::Found { .. }))
                    && AgentKind::ALL
                        .iter()
                        .any(|k| self.found_agent(*k).is_some())
            }
            CommandId::RemoveWorktree(id) => self.removable_worktree(id).is_some(),
            CommandId::WorktreeTerminal(i) => self.worktree_path(i).is_some(),
            CommandId::ToggleViewer | CommandId::ToggleMonitor | CommandId::KeyboardShortcuts => {
                true
            }
            CommandId::ViewSelectedFile => self.explorer.selected().is_some_and(|e| !e.is_dir),
            CommandId::CommandPalette
            | CommandId::OpenPromptStudio
            | CommandId::TogglePromptStudioDock
            | CommandId::ToggleGitPanel
            | CommandId::GitRefresh
            | CommandId::NewTab
            | CommandId::ZoomIn
            | CommandId::ZoomOut
            | CommandId::ZoomReset
            | CommandId::About
            | CommandId::Website
            | CommandId::Quit
            | CommandId::ToggleExplorer
            | CommandId::ToggleAiPanel
            | CommandId::ToggleMissionControl
            | CommandId::AiRefresh => true,
            CommandId::GoToTerminal(id)
            | CommandId::TerminalGit(id)
            | CommandId::StopTerminal(id) => self.terminals.contains_key(&id),
            CommandId::TerminalFolder(id) => self
                .terminals
                .get(&id)
                .is_some_and(|t| t.directory().is_some()),
            CommandId::StartAgent(kind, _) => self.found_agent(kind).is_some(),
            CommandId::OpenSettings | CommandId::ReloadSettings => self.settings_path.is_some(),
            CommandId::ExplorerToggleHidden
            | CommandId::ExplorerRefresh
            | CommandId::ExplorerToggleFollow => self.panel_open(SidePanel::Explorer),
            CommandId::ExplorerParent => {
                self.panel_open(SidePanel::Explorer) && self.explorer.parent().is_some()
            }
            CommandId::ExplorerGoToFolder => true,
            CommandId::ExplorerOpenSelected => {
                self.panel_open(SidePanel::Explorer)
                    && self.explorer.selected().is_some_and(|e| e.is_dir)
            }
            CommandId::CopySelectedPath | CommandId::NewTabInSelectedFolder => {
                self.panel_open(SidePanel::Explorer) && self.explorer.selected().is_some()
            }
            CommandId::InsertSelectedPath => {
                self.panel_open(SidePanel::Explorer)
                    && self.explorer.selected().is_some()
                    && focused.is_some_and(TerminalPane::is_running)
            }
        }
    }

    /// Inserts text at the focused terminal's prompt like a paste: never followed by
    /// Enter, and multi-line text without bracketed paste asks for confirmation.
    fn insert_text(&mut self, text: String) {
        if let Some(id) = self.workspace.focused() {
            self.insert_text_into(id, text);
        }
    }

    fn insert_text_into(&mut self, id: TermId, text: String) {
        let Some(t) = self.terminals.get(&id) else {
            return;
        };
        if paste_needs_confirmation(&text, t.modes.bracketed_paste) {
            self.pending_paste = Some((id, text));
        } else if let Some(s) = t.session() {
            s.paste(&text);
        }
    }

    /// The first search for the typed query: from the cursor upwards, most recent
    /// match first.
    fn term_find_first(&mut self) {
        let Some(query) = self.term_find.as_ref().map(|f| f.query.clone()) else {
            return;
        };
        if let Some(s) = self.focused().and_then(TerminalPane::session) {
            s.clear_selection();
        }
        let outcome = self.term_search(&query, true);
        if let Some(f) = &mut self.term_find {
            f.outcome = outcome;
            f.searched = true;
        }
    }

    /// Searches the focused terminal and refreshes its view.
    fn term_search(
        &mut self,
        query: &str,
        older: bool,
    ) -> Option<forgeterm_terminal::SearchOutcome> {
        let id = self.workspace.focused()?;
        let t = self.terminals.get_mut(&id)?;
        if query.is_empty() {
            return None;
        }
        let outcome = t.session()?.search(query, older);
        t.refresh(&self.palette);
        Some(outcome)
    }

    /// Help → Keyboard Shortcuts: every command with its keys (from the live
    /// keymap, so [keys] changes show) and its name for settings.toml.
    fn view_shortcuts(&self) -> Element<'_, Message> {
        let mut ids: Vec<CommandId> = STATIC_COMMANDS.to_vec();
        ids.push(CommandId::CommandPalette);
        ids.extend((0..self.shells.len().min(9)).map(CommandId::NewTabWithShell));
        for k in AgentKind::ALL {
            ids.push(CommandId::StartAgent(k, false));
            ids.push(CommandId::StartAgent(k, true));
        }
        ids.sort_by_key(|id| id.category());
        let mut list = column![].spacing(2);
        let mut last_category = "";
        for id in ids {
            if id.category() != last_category {
                last_category = id.category();
                list = list.push(
                    container(text(last_category).size(11).color(theme::TEXT_MUTED)).padding(
                        iced::Padding {
                            top: 8.0,
                            right: 0.0,
                            bottom: 2.0,
                            left: 0.0,
                        },
                    ),
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
                        .size(13)
                        .color(theme::TEXT)
                        .width(Length::FillPortion(5)),
                    text(if keys.is_empty() {
                        "—".to_owned()
                    } else {
                        keys.join("  ")
                    })
                    .size(12)
                    .color(if keys.is_empty() {
                        theme::DISABLED
                    } else {
                        theme::CYAN
                    })
                    .width(Length::FillPortion(4)),
                    text(id.config_name())
                        .size(11)
                        .font(crate::terminal_view::TERMINAL_FONT)
                        .color(theme::TEXT_MUTED)
                        .width(Length::FillPortion(4)),
                ]
                .spacing(10)
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
        .size(12)
        .color(theme::TEXT_MUTED);
        let card = container(
            column![
                row![
                    text("Keyboard Shortcuts").size(18).color(theme::TEXT),
                    space::horizontal(),
                    button(text("Close (Esc)").size(12))
                        .on_press(Message::ShortcutsClose)
                        .padding([5, 12])
                        .style(|_, status| ghost_button(status, theme::CYAN)),
                ]
                .align_y(iced::Alignment::Center),
                text("Change them in settings.toml under [keys]: \"Ctrl+K\" = \"command_palette\", or \"none\" to remove one.")
                    .size(12)
                    .color(theme::TEXT_MUTED),
                alt_n,
                scrollable(list).height(Length::Fill),
            ]
            .spacing(8),
        )
        .padding(18)
        .width(Length::Fixed(760.0))
        .height(Length::Fill)
        .max_height(680.0)
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
            .padding(24)
            .style(|_| container::Style {
                background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.35).into()),
                ..Default::default()
            });
        opaque(mouse_area(backdrop).on_press(Message::ShortcutsClose))
    }

    // ---- mission control ------------------------------------------------------------

    /// Mission Control or the Git Control Center covers the terminals.
    fn main_covered(&self) -> bool {
        self.mission.visible || self.git_dash.visible || self.compare.visible
    }

    // ---- git control center ---------------------------------------------------------

    /// Folder of the worktree with this id in the dashboard's list, when it is
    /// still there and usable.
    fn worktree_path(&self, id: u64) -> Option<PathBuf> {
        let list = self.git_dash.data.as_ref()?.worktrees.as_ref().ok()?;
        let info = list
            .iter()
            .find(|i| git_dash::worktree_id(&i.worktree.path) == id)?;
        let usable = !info.worktree.bare
            && !matches!(
                info.changes,
                WorktreeChanges::Missing | WorktreeChanges::NotLocal
            );
        usable.then(|| info.worktree.path.clone())
    }

    /// Branch names known from the dashboard or the Git panel (for suggestions).
    fn branch_names(&self) -> Vec<String> {
        let from = |b: &Result<Vec<forgeterm_git::BranchRef>, String>| {
            b.as_ref()
                .map(|l| l.iter().map(|b| b.name.clone()).collect::<Vec<_>>())
                .ok()
        };
        self.git_dash
            .data
            .as_ref()
            .and_then(|d| from(&d.branches))
            .or_else(|| self.git.branches.as_ref().and_then(from))
            .unwrap_or_default()
    }

    /// A linked worktree that may be removed: not the main one, not the one shown,
    /// readable, and no agent of this window working in it.
    fn removable_worktree(&self, id: u64) -> Option<(PathBuf, PathBuf, Option<String>)> {
        let data = self.git_dash.data.as_ref()?;
        let list = data.worktrees.as_ref().ok()?;
        let (index, info) = list
            .iter()
            .enumerate()
            .find(|(_, i)| git_dash::worktree_id(&i.worktree.path) == id)?;
        let w = &info.worktree;
        let shown = git_dash::path_key(&w.path) == git_dash::path_key(&data.root);
        // The deepest worktree containing each terminal (a worktree may sit in another).
        let paths: Vec<PathBuf> = list.iter().map(|i| i.worktree.path.clone()).collect();
        let terminals = self
            .terminals
            .values()
            .filter_map(TerminalPane::directory)
            .filter(|d| git_dash::worktree_of(d, &paths) == Some(index))
            .count();
        (git_dash::removal(index == 0, shown, info, terminals) == git_dash::Removal::Allowed)
            .then(|| (data.root.clone(), w.path.clone(), w.branch.clone()))
    }

    /// Reloads the dashboard's data when it is open and out of date: a different
    /// status than at the last load (a commit, stage or edit), another repository,
    /// or older than 10 s (other worktrees, commits made elsewhere). `force`: always.
    fn load_git_dash(&mut self, force: bool) -> Task<Message> {
        if !self.git_dash.visible {
            return Task::none();
        }
        if self.git_dash.busy {
            self.git_dash.reload_pending |= force;
            return Task::none();
        }
        let Some(Repo::Found { root, status }) = self.git.repo.as_ref() else {
            return Task::none();
        };
        let fresh =
            self.git_dash.data.as_ref().is_some_and(|d| {
                &d.root == root && d.loaded_at.elapsed() < Duration::from_secs(10)
            }) && self.git_dash.loaded_for.as_ref() == Some(status);
        if fresh && !force {
            return Task::none();
        }
        self.git_dash.busy = true;
        self.git_dash.loaded_for = Some(status.clone());
        let root = root.clone();
        let trusted: Vec<PathBuf> = self.git.trusted.iter().cloned().collect();
        background(
            move || Box::new(load_dashboard(root, &trusted)),
            Message::GitDashLoaded,
        )
    }

    // ---- compare worktrees ----------------------------------------------------------

    /// Reads the Compare page for the shown repository (background).
    fn load_compare(&mut self) -> Task<Message> {
        let Some(Repo::Found { root, .. }) = self.git.repo.as_ref() else {
            return Task::none();
        };
        self.compare.seq += 1;
        self.compare.busy = true;
        let (seq, root) = (self.compare.seq, root.clone());
        let (a, b) = (self.compare.a.clone(), self.compare.b.clone());
        let trusted: Vec<PathBuf> = self.git.trusted.iter().cloned().collect();
        background(
            move || Box::new(compare_view::load(seq, root, a, b, &trusted)),
            Message::CompareLoaded,
        )
    }

    /// Other worktrees chosen: the file shown belonged to the old pair.
    fn compare_sides_changed(&mut self) -> Task<Message> {
        self.compare.selected = None;
        self.compare.diff = None;
        self.load_compare()
    }

    /// Side-by-side diff of `path` between A and B. `reset`: a new selection
    /// (show "Reading…"); otherwise the old rows stay until the new ones arrive.
    fn load_compare_diff(&mut self, path: String, reset: bool) -> Task<Message> {
        let Some(data) = &self.compare.data else {
            return Task::none();
        };
        let Some([a, b]) = self.compare.paths() else {
            return Task::none();
        };
        let root = data.root.clone();
        self.compare.selected = Some(path.clone());
        if reset || self.compare.diff.as_ref().is_none_or(|d| d.path != path) {
            self.compare.diff = Some(compare_view::DiffState {
                path: path.clone(),
                result: None,
            });
        }
        background(
            move || {
                let r = compare_view::load_diff(&root, &a, &b, &path);
                ([a, b], path, r)
            },
            |(pair, path, r)| Message::CompareDiffLoaded(pair, path, r),
        )
    }

    /// Agents of this window per side (deepest worktree of their folder).
    fn compare_agents(&self) -> [Vec<compare_view::AgentIn>; 2] {
        let mut out: [Vec<compare_view::AgentIn>; 2] = Default::default();
        let Some(paths) = self.compare.paths() else {
            return out;
        };
        for (tab, t) in self.workspace.tabs.iter().enumerate() {
            for id in t.terminals() {
                if let Some(term) = self.terminals.get(&id)
                    && let (Some(kind), Some(dir)) = (term.agent, term.directory())
                    && let Some(side) = git_dash::worktree_of(dir, &paths)
                {
                    out[side].push(compare_view::AgentIn { id, kind, tab });
                }
            }
        }
        out
    }

    /// A tab with a shell in A beside one in B; the test command is typed into each
    /// once its prompt shows, never followed by Enter.
    fn open_compare_tests(&mut self) -> Task<Message> {
        let Some([a, b]) = self.compare.paths() else {
            return Task::none();
        };
        let command = self.compare.test_command.trim().to_owned();
        if command.is_empty() {
            return Task::none();
        }
        if let Some(p) = [&a, &b].into_iter().find(|p| {
            !p.to_str().is_some_and(forgeterm_terminal::is_local_path)
                || !forgeterm_fs::resolves_locally(p)
                || !p.is_dir()
        }) {
            self.notice = Some(format!(
                "No test terminals: {} is not a local folder.",
                p.display()
            ));
            return Task::none();
        }
        let Some(shell) = self.shell_at(None) else {
            self.error = Some("No supported shell was found on this system.".into());
            return Task::none();
        };
        self.compare.visible = false;
        let ia = self.spawn_terminal(shell.clone(), Some(a));
        self.workspace.push_tab(ia);
        let new_b = self.spawn_terminal(shell, Some(b));
        let split = self
            .workspace
            .active_tab_mut()
            .and_then(|tab| tab.split(Axis::Vertical, new_b));
        let ib = if split.is_some() {
            Some(new_b)
        } else {
            self.terminals.remove(&new_b);
            None
        };
        let since = std::time::Instant::now();
        for id in std::iter::once(ia).chain(ib) {
            self.pending_inserts.push(PendingInsert {
                id,
                text: command.clone(),
                since,
            });
        }
        self.compare.test_terms = [Some(ia), ib];
        self.focus_terminal(ia);
        self.after_layout_change()
    }

    /// Types waiting test commands into shells that are ready (no Enter).
    fn process_pending_inserts(&mut self) {
        let now = std::time::Instant::now();
        for p in std::mem::take(&mut self.pending_inserts) {
            let Some(t) = self.terminals.get(&p.id) else {
                continue;
            };
            let Some(s) = t.session() else {
                // Not started yet (it starts at its first layout): keep waiting.
                if t.is_awaiting_size()
                    && now.duration_since(p.since) <= compare_view::INSERT_GIVE_UP
                {
                    self.pending_inserts.push(p);
                }
                continue;
            };
            if compare_view::ready_for_insert(s.at_prompt(), t.since_output(now)) {
                self.insert_text_into(p.id, p.text);
            } else if now.duration_since(p.since) > compare_view::INSERT_GIVE_UP {
                self.notice = Some(format!(
                    "{} showed no prompt within {} s: type the test command yourself.",
                    t.display_title(),
                    compare_view::INSERT_GIVE_UP.as_secs()
                ));
            } else {
                self.pending_inserts.push(p);
            }
        }
    }

    /// The Compare prompt at the first agent of each side, like a paste, no Enter.
    fn insert_compare_prompt(&mut self) {
        let raw = self.compare.prompt.text();
        let text = raw.trim_end_matches(['\n', '\r']).to_owned();
        if text.trim().is_empty() {
            return;
        }
        let agents = self.compare_agents();
        let (mut done, mut skipped) = (Vec::new(), Vec::new());
        for (label, list) in [("A", &agents[0]), ("B", &agents[1])] {
            let Some(ag) = list.first() else {
                skipped.push(format!("{label} has no agent"));
                continue;
            };
            let Some(t) = self.terminals.get(&ag.id) else {
                continue;
            };
            let name = format!("{} (tab {})", ag.kind.name(), ag.tab + 1);
            match t.session() {
                Some(_) if paste_needs_confirmation(&text, t.modes.bracketed_paste) => {
                    skipped.push(format!(
                        "{name} has not enabled bracketed paste, so the lines would run one by one"
                    ));
                }
                Some(s) => {
                    s.paste(&text);
                    done.push(name);
                }
                None => skipped.push(format!("{name} has ended")),
            }
        }
        let mut notice = if done.is_empty() {
            "Nothing inserted.".to_owned()
        } else {
            format!(
                "Inserted into {}, without Enter: read it there and press Enter yourself.",
                done.join(" and ")
            )
        };
        if !skipped.is_empty() {
            notice.push_str(&format!(" Not inserted: {}.", skipped.join("; ")));
        }
        self.notice = Some(notice);
    }

    /// What a test terminal shows, from its shell's command marks.
    fn compare_test_state(&self, id: Option<TermId>) -> (Option<TermId>, compare_view::TestState) {
        use compare_view::TestState;
        use forgeterm_terminal::marks::CommandState;
        let Some((id, t)) = id.and_then(|id| self.terminals.get(&id).map(|t| (id, t))) else {
            return (None, TestState::None);
        };
        if self.pending_inserts.iter().any(|p| p.id == id) {
            return (Some(id), TestState::Inserting);
        }
        let Some(s) = t.session() else {
            return (Some(id), TestState::Ended);
        };
        let state = match s.last_run() {
            Some(r) => match r.state {
                CommandState::Running => TestState::Running(r.command),
                CommandState::Finished(code) => TestState::Finished(r.command, code),
                CommandState::AtPrompt => TestState::NothingYet,
            },
            // cmd marks prompts but not commands: it never reports an exit code.
            None if s.at_prompt() && t.shell.kind != forgeterm_terminal::ShellKind::Cmd => {
                TestState::NothingYet
            }
            None => TestState::NotReported,
        };
        (Some(id), state)
    }

    fn view_compare(&self) -> Element<'_, Message> {
        compare_view::view(compare_view::View {
            repo: self.git.repo.as_ref(),
            compare: &self.compare,
            agents: self.compare_agents(),
            tests: [
                self.compare_test_state(self.compare.test_terms[0]),
                self.compare_test_state(self.compare.test_terms[1]),
            ],
            now: std::time::Instant::now(),
        })
    }

    fn view_git_dash(&self) -> Element<'_, Message> {
        let mut agents = Vec::new();
        for (tab, t) in self.workspace.tabs.iter().enumerate() {
            for id in t.terminals() {
                if let Some(term) = self.terminals.get(&id)
                    && let (Some(kind), Some(dir)) = (term.agent, term.directory())
                {
                    agents.push(git_dash::AgentAt {
                        id,
                        kind,
                        tab,
                        dir: dir.to_path_buf(),
                    });
                }
            }
        }
        git_dash::view(git_dash::View {
            repo: self.git.repo.as_ref(),
            dash: &self.git_dash,
            agents,
            terminal_dirs: self
                .terminals
                .values()
                .filter_map(|t| t.directory().map(Path::to_path_buf))
                .collect(),
            panel_hint: self.keymap.hint(CommandId::ToggleGitPanel),
            compare_hint: self.keymap.hint(CommandId::ToggleCompare),
            dir: self.focused().and_then(TerminalPane::directory),
            now: std::time::Instant::now(),
        })
    }

    fn toggle_mission(&mut self) -> Task<Message> {
        self.mission.visible = !self.mission.visible;
        if self.mission.visible {
            self.git_dash.visible = false;
            self.compare.visible = false;
        }
        if !self.mission.visible {
            self.refresh_visible();
            return Task::none();
        }
        // Samples are only taken while it is open: an old history would join
        // measurements minutes apart as if they were consecutive.
        self.mission.cpu.clear();
        // Fresh previews right away (hidden terminals only mark themselves dirty).
        for t in self.terminals.values_mut().filter(|t| t.agent.is_some()) {
            t.refresh(&self.palette);
        }
        let order = self.mission_order();
        if !self.mission.selected.is_some_and(|id| order.contains(&id)) {
            self.mission.selected = order.first().copied();
        }
        Task::batch(
            [self.read_mission_heads()]
                .into_iter()
                .chain(self.sample_monitor()),
        )
    }

    fn go_to_terminal(&mut self, id: TermId) {
        if !self.terminals.contains_key(&id) {
            return;
        }
        // Whichever page covers the terminals (Mission Control, Git Control Center).
        self.mission.visible = false;
        self.git_dash.visible = false;
        self.compare.visible = false;
        self.mission.selected = Some(id);
        self.focus_terminal(id);
        self.refresh_visible();
    }

    /// Agent terminals in dashboard order: most urgent first, then tab order.
    fn mission_order(&self) -> Vec<TermId> {
        let now = std::time::Instant::now();
        self.mission_cards(now).iter().map(|c| c.id).collect()
    }

    fn mission_cards(&self, now: std::time::Instant) -> Vec<mission_view::Card<'_>> {
        use forgeterm_agents::mission::{Reported, classify};
        use forgeterm_terminal::status::ProgramState;
        let focused = self.workspace.focused();
        let usage = self.monitor.last.as_ref();
        let mut cards = Vec::new();
        for (tab, t_ids) in self
            .workspace
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| (i, tab.terminals().collect::<Vec<_>>()))
        {
            for id in t_ids {
                let Some(t) = self.terminals.get(&id) else {
                    continue;
                };
                let (Some(kind), Some(root)) = (t.agent, t.directory()) else {
                    continue;
                };
                let record = t.program_status();
                let reported = record.as_ref().map(|r| match r.state {
                    ProgramState::Working => Reported::Working,
                    ProgramState::Blocked => Reported::Blocked,
                    ProgramState::Idle => Reported::Idle,
                    ProgramState::Done => Reported::Done,
                    ProgramState::Error => Reported::Error,
                });
                let message = record.and_then(|r| r.msg.or(r.title));
                let pid = t.session().and_then(|s| s.pid());
                cards.push(mission_view::Card {
                    id,
                    kind,
                    tab,
                    title: t.display_title(),
                    root,
                    head: self.mission.heads.get(root).and_then(Option::as_ref),
                    status: classify(&t.process(), reported, t.bell),
                    message,
                    elapsed: t.elapsed(now),
                    since_output: t.since_output(now),
                    usage: pid.and_then(|p| usage?.shells.get(&p)),
                    cpu: self.mission.cpu.get(&id).map(|(_, h)| h),
                    snapshot: &t.snapshot,
                    cache: &t.preview_cache,
                    selected: self.mission.selected == Some(id),
                    focused: focused == Some(id),
                });
            }
        }
        // Stable: equal urgency keeps tab order.
        cards.sort_by_key(|c| c.status.urgency());
        cards
    }

    fn view_mission(&self) -> Element<'_, Message> {
        let now = std::time::Instant::now();
        let start_root = self.ai_context.as_ref().map(|c| c.root.as_path());
        mission_view::view(mission_view::View {
            cards: self.mission_cards(now),
            mission: &self.mission,
            agents: &self.agents,
            start_root,
            now,
            worktree_ok: self.is_applicable(CommandId::NewAgentWorktree),
            back_hint: self.keymap.hint(CommandId::ToggleMissionControl),
        })
    }

    /// Esc closes, arrows / Tab move the selection, Enter opens the terminal.
    /// Other keys are swallowed: the terminals are hidden behind the dashboard.
    fn mission_key(&mut self, key: &keyboard::Key, mods: keyboard::Modifiers) -> Task<Message> {
        let order = self.mission_order();
        let pos = self
            .mission
            .selected
            .and_then(|id| order.iter().position(|&o| o == id));
        let step = |forward: bool| -> Option<TermId> {
            if order.is_empty() {
                return None;
            }
            let i = match (pos, forward) {
                (None, _) => 0,
                (Some(i), true) => (i + 1) % order.len(),
                (Some(i), false) => (i + order.len() - 1) % order.len(),
            };
            Some(order[i])
        };
        match key {
            keyboard::Key::Named(Named::Escape) => return self.toggle_mission(),
            keyboard::Key::Named(Named::ArrowRight | Named::ArrowDown) => {
                self.mission.selected = step(true);
            }
            keyboard::Key::Named(Named::ArrowLeft | Named::ArrowUp) => {
                self.mission.selected = step(false);
            }
            keyboard::Key::Named(Named::Tab) => self.mission.selected = step(!mods.shift()),
            keyboard::Key::Named(Named::Enter) => {
                if let Some(id) = self.mission.selected.filter(|id| order.contains(id)) {
                    return self.run_command(CommandId::GoToTerminal(id));
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// Reads the branch of every agent's project in the background (no git process).
    fn read_mission_heads(&mut self) -> Task<Message> {
        if self.mission.heads_busy {
            return Task::none();
        }
        let mut roots: Vec<PathBuf> = self
            .terminals
            .values()
            .filter(|t| t.agent.is_some())
            .filter_map(|t| t.directory())
            .filter(|d| d.to_str().is_some_and(forgeterm_terminal::is_local_path))
            .map(Path::to_path_buf)
            .collect();
        roots.sort();
        roots.dedup();
        if roots.is_empty() {
            return Task::none();
        }
        self.mission.heads_busy = true;
        background(
            move || {
                roots
                    .into_iter()
                    .map(|r| {
                        let head = forgeterm_git::head(&r);
                        (r, head)
                    })
                    .collect()
            },
            Message::MissionHeads,
        )
    }

    fn record_mission_cpu(&mut self, sample: &forgeterm_system::Sample) {
        let agents: Vec<(TermId, u32)> = self
            .terminals
            .values()
            .filter(|t| t.agent.is_some())
            .filter_map(|t| Some((t.id, t.session()?.pid()?)))
            .collect();
        for &(id, pid) in &agents {
            if let Some(cpu) = sample.shells.get(&pid).and_then(|u| u.cpu_percent) {
                self.mission.record_cpu(id, pid, cpu);
            }
        }
        // Ended or closed agents keep no (frozen) history.
        self.mission
            .cpu
            .retain(|id, _| agents.iter().any(|(a, _)| a == id));
    }

    /// Remembers an agent session that is going away, for "recently ended".
    fn note_ended(&mut self, id: TermId) {
        let Some(t) = self.terminals.get(&id) else {
            return;
        };
        let Some(kind) = t.agent else {
            return;
        };
        let how = match &t.status {
            Status::Running => EndKind::Closed,
            Status::Exited(code) => EndKind::Exited(*code),
            Status::Failed(_) => EndKind::FailedToStart,
        };
        let now = std::time::Instant::now();
        let project = t
            .directory()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.mission.record_ended(Ended {
            kind,
            project,
            ran: t.elapsed(now),
            at: now,
            how,
        });
        if self.mission.selected == Some(id) {
            self.mission.selected = None;
        }
    }

    // ---- monitor --------------------------------------------------------------------

    /// (label, shell pid) of every running terminal, in tab order.
    fn monitor_rows(&self) -> Vec<(String, u32)> {
        let mut rows = Vec::new();
        for (i, tab) in self.workspace.tabs.iter().enumerate() {
            for id in tab.terminals() {
                if let Some(t) = self.terminals.get(&id)
                    && let Some(pid) = t.session().and_then(|s| s.pid())
                {
                    rows.push((format!("{} (tab {})", t.display_title(), i + 1), pid));
                }
            }
        }
        rows
    }

    /// Starts a measurement when the monitor is open and none is running.
    fn sample_monitor(&mut self) -> Option<Task<Message>> {
        if !(self.panel_open(SidePanel::Monitor) || self.mission.visible) || self.monitor.busy {
            return None;
        }
        self.monitor.busy = true;
        let pids: Vec<u32> = self.monitor_rows().into_iter().map(|(_, p)| p).collect();
        let sampler = self.monitor.sampler.clone();
        Some(background(
            move || {
                let mut s = sampler
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                Box::new(s.sample(&pids))
            },
            Message::MonitorSampled,
        ))
    }

    // ---- session restore ----------------------------------------------------------

    fn session_path(&self) -> Option<PathBuf> {
        self.settings_path
            .as_deref()
            .and_then(forgeterm_settings::session::path_for)
    }

    /// Reopens the last session's tabs, splits and viewer files when enabled and
    /// available; otherwise one tab with the default shell.
    fn restore_or_new_tab(&mut self) -> Task<Message> {
        let saved = self
            .session_path()
            .filter(|_| self.settings.workspace.restore_session)
            .map(|p| forgeterm_settings::session::load(&p))
            .unwrap_or_default();
        let docked = saved.prompt_studio_docked;
        let task = self.restore_tabs(saved);
        if docked {
            // Prompt Studio was open beside the terminal: open it there again (empty;
            // typed values are never saved).
            self.prompt_docked = true;
            return Task::batch([task, self.open_prompt_studio()]);
        }
        task
    }

    fn restore_tabs(&mut self, saved: forgeterm_settings::session::SavedSession) -> Task<Message> {
        // No shell at all: new_tab shows the error screen.
        if saved.tabs.is_empty() || self.shells.is_empty() {
            return self.new_tab(None, None);
        }
        for layout in &saved.tabs {
            let config = self.restore_layout(layout);
            let panes = pane_grid::State::with_configuration(config);
            let focus = panes.iter().next().map(|(p, _)| *p);
            if let Some(focus) = focus {
                self.workspace
                    .tabs
                    .push(crate::workspace::Tab { panes, focus });
            }
        }
        if self.workspace.tabs.is_empty() {
            return self.new_tab(None, None);
        }
        self.workspace.active = saved.active.min(self.workspace.tabs.len() - 1);
        let files: Vec<PathBuf> = saved
            .viewer_files
            .into_iter()
            .filter(|p| {
                forgeterm_terminal::is_local_path(&p.to_string_lossy())
                    && forgeterm_fs::resolves_locally(p)
                    && p.is_file()
            })
            .collect();
        if !files.is_empty() {
            self.viewer.visible = true;
        }
        Task::batch(files.into_iter().map(load_document))
    }

    fn restore_layout(
        &mut self,
        layout: &forgeterm_settings::session::Layout,
    ) -> pane_grid::Configuration<TermId> {
        use forgeterm_settings::session::Layout;
        match layout {
            Layout::Pane(spec) => {
                // The saved shell if it is still installed, else the default one.
                let shell = self
                    .shells
                    .iter()
                    .find(|s| s.program.eq_ignore_ascii_case(&spec.shell))
                    .cloned()
                    .or_else(|| self.shell_at(None));
                let cwd = spec.cwd.clone().filter(|d| {
                    forgeterm_terminal::is_local_path(&d.to_string_lossy())
                        && forgeterm_fs::resolves_locally(d)
                        && d.is_dir()
                });
                let id = match shell {
                    Some(shell) => self.spawn_terminal(shell, cwd),
                    None => self.next_id,
                };
                pane_grid::Configuration::Pane(id)
            }
            Layout::Split {
                side_by_side,
                ratio,
                a,
                b,
            } => pane_grid::Configuration::Split {
                axis: if *side_by_side {
                    Axis::Vertical
                } else {
                    Axis::Horizontal
                },
                ratio: *ratio,
                a: Box::new(self.restore_layout(a)),
                b: Box::new(self.restore_layout(b)),
            },
        }
    }

    /// The current layout as a session (agents are saved as the default shell in
    /// their folder: an agent is never started automatically).
    fn current_session(&self) -> forgeterm_settings::session::SavedSession {
        use forgeterm_settings::session::{Layout, PaneSpec, SavedSession};
        let default_program = self.shell_at(None).map(|s| s.program);
        let pane_spec = |id: &TermId| -> Option<PaneSpec> {
            let t = self.terminals.get(id)?;
            let shell = if t.agent.is_some() {
                default_program.clone()?
            } else {
                t.shell.program.clone()
            };
            Some(PaneSpec {
                shell,
                cwd: t.directory().map(Path::to_path_buf),
            })
        };
        fn walk(
            node: &pane_grid::Node,
            panes: &pane_grid::State<TermId>,
            spec: &dyn Fn(&TermId) -> Option<PaneSpec>,
        ) -> Option<Layout> {
            match node {
                pane_grid::Node::Pane(p) => panes.get(*p).and_then(spec).map(Layout::Pane),
                pane_grid::Node::Split {
                    axis, ratio, a, b, ..
                } => Some(Layout::Split {
                    side_by_side: *axis == Axis::Vertical,
                    ratio: *ratio,
                    a: Box::new(walk(a, panes, spec)?),
                    b: Box::new(walk(b, panes, spec)?),
                }),
            }
        }
        SavedSession {
            tabs: self
                .workspace
                .tabs
                .iter()
                .filter_map(|t| walk(t.panes.layout(), &t.panes, &pane_spec))
                .collect(),
            active: self.workspace.active,
            viewer_files: self
                .viewer
                .tabs
                .iter()
                .map(|t| t.doc.path.clone())
                .collect(),
            prompt_studio_docked: self.prompt_docked && self.prompt_studio.is_some(),
        }
    }

    /// Writes session.toml when it changed (and restoring is enabled).
    fn save_session(&mut self) {
        if !self.settings.workspace.restore_session {
            return;
        }
        let Some(path) = self.session_path() else {
            return;
        };
        let session = self.current_session();
        let text = forgeterm_settings::session::to_toml(&session);
        if self.saved_session.as_deref() == Some(text.as_str()) {
            return;
        }
        match forgeterm_settings::session::save(&path, &session) {
            Ok(()) => {
                self.saved_session = Some(text);
                self.session_save_error = None;
            }
            Err(e) => {
                let e = e.to_string();
                // Saving is retried at every change: report a failure once, not
                // every 2 s.
                if self.session_save_error.as_ref() != Some(&e) {
                    tracing::warn!("could not save {}: {e}", path.display());
                    self.notice = Some(format!(
                        "Could not save the session to {}: {e}. Tabs and folders will not come back at the next start until this works.",
                        path.display()
                    ));
                    self.session_save_error = Some(e);
                }
            }
        }
    }

    /// A program in a terminal set the clipboard (OSC 52). Allowed, as in other
    /// terminals, but never silently: the user sees which tab did it. Huge
    /// payloads are refused.
    fn program_clipboard(&mut self, id: TermId, text: String) -> Task<Message> {
        const MAX: usize = 1024 * 1024;
        let tab = self.workspace.locate(id).map_or(0, |(i, _)| i + 1);
        let who = self.terminals.get(&id).map_or_else(
            || "A program".to_owned(),
            |t| format!("{} (tab {tab})", t.display_title()),
        );
        if text.len() > MAX {
            self.notice = Some(format!(
                "{who} tried to put {} KB on the clipboard; refused (limit 1 MB).",
                text.len() / 1024
            ));
            return Task::none();
        }
        self.notice = Some(format!(
            "{who} copied {} character{} to the clipboard.",
            text.chars().count(),
            if text.chars().count() == 1 { "" } else { "s" }
        ));
        clipboard::write(text)
    }

    // ---- closing ------------------------------------------------------------------

    /// Closes at once when nothing runs in the affected terminals; otherwise asks
    /// first, naming the programs (looked up off the UI thread).
    fn request_close(&mut self, target: CloseTarget) -> Task<Message> {
        let ids: Vec<TermId> = match &target {
            CloseTarget::Quit => self.terminals.keys().copied().collect(),
            CloseTarget::Tab(ids) => ids.clone(),
            CloseTarget::Pane(id) => vec![*id],
        };
        let shells: Vec<(u32, String)> = ids
            .iter()
            .filter_map(|id| {
                let t = self.terminals.get(id)?;
                let pid = t.session()?.pid()?;
                let tab = self.workspace.locate(*id).map_or(0, |(i, _)| i + 1);
                Some((pid, format!("{} (tab {tab})", t.display_title())))
            })
            .collect();
        // An agent runs as the terminal's own process (no child of a shell), so the
        // process check would not see it: ending a session always asks.
        let agents: Vec<String> = ids
            .iter()
            .filter_map(|id| {
                let t = self.terminals.get(id).filter(|t| t.is_running())?;
                let kind = t.agent?;
                let tab = self.workspace.locate(*id).map_or(0, |(i, _)| i + 1);
                Some(format!("{kind} session (tab {tab})"))
            })
            .collect();
        if shells.is_empty() && agents.is_empty() {
            return self.perform_close(target);
        }
        background(
            move || {
                let pids: Vec<u32> = shells.iter().map(|(p, _)| *p).collect();
                let running = forgeterm_system::running_programs(&pids);
                let mut found = agents;
                for (pid, label) in &shells {
                    if let Some(programs) = running.get(pid).filter(|p| !p.is_empty()) {
                        let names: Vec<&str> = programs.iter().map(|p| p.name.as_str()).collect();
                        found.push(format!("{} in {label}", names.join(", ")));
                    }
                }
                (target, found)
            },
            |(target, found)| Message::CloseChecked(target, found),
        )
    }

    fn perform_close(&mut self, target: CloseTarget) -> Task<Message> {
        match target {
            // Same as closing the window: sessions are dropped, shells are shut down.
            CloseTarget::Quit => {
                self.save_session();
                iced::exit()
            }
            CloseTarget::Tab(ids) => {
                let index = ids
                    .first()
                    .and_then(|id| self.workspace.locate(*id))
                    .map(|(i, _)| i);
                match index {
                    Some(i) => self.close_tab(i),
                    None => Task::none(),
                }
            }
            CloseTarget::Pane(id) => self.close_terminal(id),
        }
    }

    // ---- viewer -------------------------------------------------------------------

    fn viewer_msg(&mut self, msg: ViewerMsg) -> Task<Message> {
        let v = &mut self.viewer;
        match msg {
            ViewerMsg::Open(path) => {
                v.visible = true;
                v.error = None;
                if let Some(i) = v.index_of(&path) {
                    v.active = i;
                    v.focused = true;
                    self.explorer_focused = false;
                    if let Some((p, line)) = v.pending_line.take()
                        && p == path
                    {
                        v.tabs[i].go_to_line(line);
                    }
                    if v.find_open {
                        v.refresh_matches();
                    }
                    return Task::none();
                }
                return load_document(path);
            }
            ViewerMsg::Loaded(path, result) => match result {
                Err(e) => {
                    // A later, unrelated open of this file must not jump to the line.
                    v.pending_line.take_if(|(p, _)| *p == path);
                    if let Some(i) = v.index_of(&path) {
                        v.tabs[i].reloading = false;
                    } else {
                        v.error = Some(e);
                    }
                }
                Ok(doc) => {
                    let index = match v.index_of(&path) {
                        Some(i) => {
                            v.tabs[i].replace(*doc);
                            i
                        }
                        None => {
                            v.tabs.push(ViewerTab::new(*doc));
                            v.active = v.tabs.len() - 1;
                            v.focused = true;
                            self.explorer_focused = false;
                            v.tabs.len() - 1
                        }
                    };
                    if index == v.active && v.find_open {
                        v.refresh_matches();
                    }
                    let pending = v.pending_line.take_if(|(p, _)| *p == path);
                    let tab = &mut v.tabs[index];
                    if let Some((_, line)) = pending {
                        tab.go_to_line(line);
                    }
                    if tab.doc.highlightable() && tab.doc.syntax != "Plain Text" {
                        tab.highlighting = true;
                        let doc = tab.doc.clone();
                        return background(
                            move || {
                                let spans = forgeterm_viewer::highlight(&doc);
                                (doc.path, doc.modified, spans)
                            },
                            |(path, modified, spans)| {
                                Message::Viewer(ViewerMsg::Highlighted(path, modified, spans))
                            },
                        );
                    }
                }
            },
            ViewerMsg::Highlighted(path, modified, spans) => {
                // Only for the version that is still shown.
                if let Some(t) = v
                    .tabs
                    .iter_mut()
                    .find(|t| t.doc.path == path && t.doc.modified == modified)
                {
                    t.spans = spans;
                    t.highlighting = false;
                    t.cache.clear();
                }
            }
            ViewerMsg::Scroll(lines) => {
                if let Some(t) = v.tab_mut() {
                    t.scroll_by(lines);
                }
            }
            ViewerMsg::ScrollFraction(f) => {
                v.focused = true;
                self.explorer_focused = false;
                if let Some(t) = v.tab_mut() {
                    t.scroll_to_fraction(f);
                }
            }
            ViewerMsg::CenterOn(line) => {
                v.focused = true;
                self.explorer_focused = false;
                if let Some(t) = v.tab_mut() {
                    t.center_on(line);
                }
            }
            ViewerMsg::Rows(rows) => {
                if let Some(t) = v.tab_mut() {
                    t.rows = rows.max(1);
                    t.top = t.top.min(t.doc.line_count().saturating_sub(t.rows));
                    t.cache.clear();
                }
            }
            ViewerMsg::SelectStart(line, extend) => {
                v.focused = true;
                self.explorer_focused = false;
                if let Some(t) = v.tab_mut() {
                    t.selection = match (extend, t.selection) {
                        (true, Some((anchor, _))) => Some((anchor, line)),
                        _ => Some((line, line)),
                    };
                    t.cache.clear();
                }
            }
            ViewerMsg::SelectTo(line) => {
                if let Some(t) = v.tab_mut() {
                    let line = line.min(t.doc.line_count().saturating_sub(1));
                    if let Some((anchor, _)) = t.selection {
                        t.selection = Some((anchor, line));
                    }
                    t.reveal(line);
                }
            }
            ViewerMsg::SelectTab(i) => {
                if i < v.tabs.len() {
                    v.active = i;
                    v.focused = true;
                    self.explorer_focused = false;
                    if v.find_open {
                        v.refresh_matches();
                    }
                }
            }
            ViewerMsg::CloseTab(i) => {
                if i < v.tabs.len() {
                    v.tabs.remove(i);
                    if v.active >= v.tabs.len() {
                        v.active = v.tabs.len().saturating_sub(1);
                    } else if i < v.active {
                        v.active -= 1;
                    }
                    if v.tabs.is_empty() {
                        v.focused = false;
                    }
                    v.refresh_matches();
                }
            }
            ViewerMsg::FindQuery(q) => {
                v.query = q;
                v.refresh_matches();
            }
            ViewerMsg::FindNext => v.step_match(true),
            ViewerMsg::FindPrev => v.step_match(false),
            ViewerMsg::OpenFind => {
                v.visible = true;
                v.find_open = true;
                v.refresh_matches();
                return iced::widget::operation::focus(viewer_view::FIND_INPUT_ID);
            }
            ViewerMsg::CloseFind => {
                v.find_open = false;
                v.focused = !v.tabs.is_empty();
                if let Some(t) = v.tab_mut() {
                    t.cache.clear();
                }
            }
            ViewerMsg::CopySelection => {
                if let Some(t) = v.tab()
                    && let Some((a, b)) = t.selected_range()
                {
                    let text = forgeterm_viewer::copy_lines(&t.doc, a, b);
                    let text = if cfg!(windows) {
                        text.replace('\n', "\r\n")
                    } else {
                        text
                    };
                    return clipboard::write(text);
                }
            }
            ViewerMsg::CopyPath => {
                if let Some(t) = v.tab() {
                    return clipboard::write(t.doc.path.display().to_string());
                }
            }
            ViewerMsg::InsertPath => {
                if let Some(path) = v.tab().map(|t| t.doc.path.clone()) {
                    self.insert_paths(&[path]);
                }
            }
            ViewerMsg::Reload => {
                if let Some(t) = v.tab_mut() {
                    t.reloading = true;
                    return load_document(t.doc.path.clone());
                }
            }
            ViewerMsg::ClosePanel => {
                v.visible = false;
                v.focused = false;
            }
            ViewerMsg::Escape => {
                let had_selection = v.tab().is_some_and(|t| t.selection.is_some());
                if let Some(t) = v.tab_mut().filter(|_| had_selection) {
                    t.selection = None;
                    t.cache.clear();
                } else {
                    v.focused = false;
                }
            }
            ViewerMsg::SelectAll => {
                if let Some(t) = v.tab_mut() {
                    t.selection = Some((0, t.doc.line_count().saturating_sub(1)));
                    t.cache.clear();
                }
            }
        }
        Task::none()
    }

    // ---- git ----------------------------------------------------------------------

    /// Folder git status is shown for: the focused terminal's.
    fn git_dir(&self) -> PathBuf {
        self.focused_cwd().unwrap_or_else(|| self.start_dir.clone())
    }

    /// Refreshes when the focused terminal moved to another folder.
    fn sync_git(&mut self) -> Option<Task<Message>> {
        let dir = self.git_dir();
        if self.git.dir.as_ref() == Some(&dir) || self.git.refreshing.as_ref() == Some(&dir) {
            return None;
        }
        Some(self.start_git_refresh(dir, None))
    }

    /// Refreshes the current folder. The periodic tick skips while a refresh is
    /// running; `force` (user action, panel opened) does not.
    fn refresh_git(&mut self, force: bool) -> Task<Message> {
        if self.git.refreshing.is_some() && !force {
            return Task::none();
        }
        let dir = self.git_dir();
        // Same folder: reuse the known root (one git call instead of two).
        let root = (self.git.dir.as_ref() == Some(&dir))
            .then(|| self.git.root().cloned())
            .flatten();
        self.start_git_refresh(dir, root)
    }

    fn start_git_refresh(&mut self, dir: PathBuf, known_root: Option<PathBuf>) -> Task<Message> {
        self.git.refreshing = Some(dir.clone());
        let with_log = self.panel_open(SidePanel::Git);
        let trusted: Vec<PathBuf> = self.git.trusted.iter().cloned().collect();
        background(
            move || git_snapshot(dir, known_root, with_log, &trusted),
            |s| Message::GitRefreshed(Box::new(s)),
        )
    }

    fn git_action(&mut self, action: GitAction) -> Task<Message> {
        let Some(root) = self.git.root().cloned() else {
            return Task::none();
        };
        if self.git.action_running {
            return Task::none();
        }
        let has_commits = self.git.status().is_some_and(|s| s.branch.has_commits());
        self.git.action_running = true;
        self.git.last_action_was_commit = matches!(action, GitAction::Commit(_));
        self.git.last_action_was_create = matches!(action, GitAction::Create(_));
        background(
            move || match action {
                GitAction::Stage(paths) => forgeterm_git::stage(&root, &paths)
                    .map(|()| format!("Staged {}", describe(&paths))),
                GitAction::Unstage(paths) => forgeterm_git::unstage(&root, &paths, has_commits)
                    .map(|()| format!("Unstaged {}", describe(&paths))),
                GitAction::Commit(message) => {
                    forgeterm_git::commit(&root, &message).map(|s| format!("Committed {s}"))
                }
                GitAction::Switch(name) => {
                    forgeterm_git::switch(&root, &name).map(|()| format!("Switched to {name}"))
                }
                GitAction::Track(remote) => forgeterm_git::track(&root, &remote)
                    .map(|()| format!("Created and switched to a branch tracking {remote}")),
                GitAction::Create(name) => forgeterm_git::create_branch(&root, &name)
                    .map(|()| format!("Created and switched to {name}")),
            },
            Message::GitActionDone,
        )
    }

    // ---- prompt studio ------------------------------------------------------------

    /// `<settings dir>/prompts`, or `None` without a config directory.
    fn prompts_dir(&self) -> Option<PathBuf> {
        self.settings_path
            .as_deref()
            .and_then(forgeterm_prompts::user_dir)
    }

    fn open_prompt_studio(&mut self) -> Task<Message> {
        self.menu = None;
        self.command_palette = None;
        let auto = self.auto_prompt_values();
        self.prompt_studio = Some(PromptStudio::new(auto));
        load_prompts(self.prompts_dir())
    }

    /// Values ForgeTerm really knows; anything unknown is simply left out.
    fn auto_prompt_values(&self) -> HashMap<String, String> {
        let dir = self.focused_cwd().unwrap_or_else(|| self.start_dir.clone());
        let root = forgeterm_agents::project_root(&dir).0;
        let mut values = HashMap::new();
        if let Some(name) = root.file_name() {
            values.insert("project".to_owned(), name.to_string_lossy().into_owned());
        }
        values.insert("project_path".to_owned(), root.display().to_string());
        values.insert("cwd".to_owned(), dir.display().to_string());
        if let Some(selection) = self
            .focused()
            .and_then(TerminalPane::session)
            .and_then(|s| s.selection_text())
        {
            values.insert("selection".to_owned(), selection);
        }
        if let Some(file) = self.explorer.selected().filter(|e| !e.is_dir) {
            let shown = file.path.strip_prefix(&root).map_or_else(
                |_| file.path.display().to_string(),
                |rel| rel.to_string_lossy().replace('\\', "/"),
            );
            values.insert("file".to_owned(), shown);
        }
        values
    }

    /// The terminal Prompt Studio inserts into: the focused one, while it runs. The
    /// floating studio is modal, so that is the terminal focused when it opened.
    fn prompt_target(&self) -> Option<&TerminalPane> {
        self.focused().filter(|t| t.is_running())
    }

    /// The agent that receives inserted command output: a running agent terminal in
    /// the active tab, else the first one in another tab.
    fn agent_target(&self) -> Option<TermId> {
        let is_agent = |id: &TermId| {
            self.terminals
                .get(id)
                .is_some_and(|t| t.agent.is_some() && t.is_running())
        };
        self.workspace
            .active_tab()
            .and_then(|tab| tab.terminals().find(is_agent))
            .or_else(|| {
                self.workspace
                    .tabs
                    .iter()
                    .flat_map(|tab| tab.terminals())
                    .find(is_agent)
            })
    }

    /// Puts the focused shell's last command output (Markdown, see
    /// `CommandOutput::to_markdown`) at the agent's prompt, like a paste and without
    /// Enter, then gives the agent the keyboard so the user can add a question.
    fn insert_last_output_into_agent(&mut self) -> Task<Message> {
        let Some(session) = self.focused().and_then(TerminalPane::session) else {
            return Task::none();
        };
        let output = match session.last_command_output() {
            Ok(out) => out,
            Err(e) => {
                self.notice = Some(format!("Nothing inserted: {e}."));
                return Task::none();
            }
        };
        let Some(target) = self.agent_target() else {
            self.notice = Some("Nothing inserted: no agent is running.".into());
            return Task::none();
        };
        if let Some((index, _)) = self.workspace.locate(target)
            && self.workspace.select(index)
        {
            self.refresh_visible();
        }
        self.focus_terminal(target);
        self.insert_text_into(target, output.to_markdown());
        unfocus_widgets()
    }

    /// Terminals the user is looking at no longer need attention.
    fn acknowledge_seen(&mut self) {
        if !self.window_focused || self.main_covered() {
            return;
        }
        let Some(tab) = self.workspace.active_tab() else {
            return;
        };
        for id in tab.terminals().collect::<Vec<_>>() {
            if let Some(t) = self.terminals.get_mut(&id) {
                t.acknowledge();
            }
        }
    }

    /// Inserts the rendered prompt into the focused terminal, without Enter, and
    /// gives that terminal the keyboard. The docked studio stays open.
    fn insert_prompt(&mut self) -> Task<Message> {
        let Some(r) = self
            .prompt_studio
            .as_ref()
            .and_then(PromptStudio::rendered)
            .filter(|r| r.missing.is_empty() && !r.text.is_empty())
        else {
            return Task::none();
        };
        // No running terminal (it ended meanwhile): the text stays, Copy still works.
        let Some(id) = self.prompt_target().map(|t| t.id) else {
            return Task::none();
        };
        if !self.prompt_docked {
            self.prompt_studio = None;
        }
        self.focus_terminal(id);
        self.insert_text_into(id, r.text);
        unfocus_widgets()
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
        // The path lands in the terminal: so does the keyboard.
        self.explorer_focused = false;
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
            CommandId::GoToTerminal(t) => match self.terminals.get(&t) {
                Some(term) => {
                    let tab = self.workspace.locate(t).map_or(0, |(i, _)| i + 1);
                    let name = term
                        .agent
                        .map_or_else(|| term.display_title(), |k| k.name().to_owned());
                    let project = term
                        .directory()
                        .and_then(Path::file_name)
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    format!("Go to {name} — {project} (tab {tab})")
                }
                None => id.static_title().to_owned(),
            },
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
        for id in self.mission_order() {
            let id = CommandId::GoToTerminal(id);
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
            (Some(err), _) => container(text(err.as_str()).color(theme::CORAL))
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
    fn tab_badge(&self, tab: &crate::workspace::Tab) -> Option<(String, Color)> {
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
            Some((ProgramState::Blocked, _)) => Some(("needs you".into(), theme::AMBER)),
            Some((ProgramState::Error, _)) => Some(("failed".into(), theme::CORAL)),
            Some((ProgramState::Done, _)) => Some(("done".into(), theme::MINT)),
            _ if bell => Some(("bell".into(), theme::AMBER)),
            Some((ProgramState::Working, Some(p))) => Some((format!("{p}%"), theme::CYAN)),
            Some((ProgramState::Working, None)) => Some(("working".into(), theme::CYAN)),
            _ => None,
        }
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
            let mut content = row![
                text("●").size(9).color(dot),
                text(label).size(12).color(if active {
                    theme::TEXT
                } else {
                    theme::TEXT_MUTED
                }),
            ]
            .spacing(7)
            .align_y(iced::Alignment::Center);
            if let Some((badge, color)) = self.tab_badge(tab) {
                content = content.push(text(badge).size(11).color(color));
            }
            let select = button(content)
                .on_press(Message::SelectTab(index))
                .padding([5, 4])
                .style(|_, _| button::Style::default());
            let close = button(text("×").size(13).color(theme::TEXT_MUTED))
                .on_press(Message::Command(CommandId::CloseTabAt(index)))
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
        let git_summary: Element<'_, Message> = match self.git.summary() {
            Some((summary, color)) => mouse_area(
                text(summary)
                    .size(12)
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
                text(status).size(12).color(color),
                text(shell).size(12).color(theme::TEXT),
                text(cwd)
                    .size(12)
                    .color(theme::TEXT)
                    .wrapping(text::Wrapping::None),
                git_summary,
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

/// A dismissible one-line problem report.
fn notice_banner(message: &str) -> Element<'_, Message> {
    container(
        row![
            text("!").size(15).color(theme::AMBER),
            text(message).size(12).color(theme::TEXT),
            space::horizontal(),
            button(text("Dismiss").size(12))
                .on_press(Message::DismissNotice)
                .padding([4, 10])
                .style(|_, status| ghost_button(status, theme::CYAN)),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center),
    )
    .padding([6, 14])
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

/// Find bar above the terminals (Ctrl+Shift+F).
fn term_find_bar(f: &TermFind) -> Element<'_, Message> {
    let status = match (f.query.is_empty(), f.outcome) {
        (true, _) => "Type to search the output and scrollback",
        (false, Some(forgeterm_terminal::SearchOutcome::NotFound)) => "Not found",
        (false, Some(forgeterm_terminal::SearchOutcome::Found)) => {
            "Found · Enter/F3 older · Shift+F3 newer · Ctrl+C copies"
        }
        (false, None) => "",
    };
    let not_found = f.outcome == Some(forgeterm_terminal::SearchOutcome::NotFound);
    let small = |label: &'static str, msg: Message| {
        button(text(label).size(12))
            .on_press(msg)
            .padding([3, 9])
            .style(|_, status| ghost_button(status, theme::CYAN))
    };
    container(
        row![
            text("Find").size(12).color(theme::TEXT_MUTED),
            text_input("in terminal output", &f.query)
                .id(iced::widget::Id::new(TERM_FIND_ID))
                .on_input(Message::TermFindQuery)
                .on_submit(Message::TermFindStep(true))
                .size(13)
                .padding([4, 8])
                .width(Length::Fixed(320.0))
                .style(move |_, status| text_input::Style {
                    background: theme::OBSIDIAN.into(),
                    border: Border {
                        color: if not_found {
                            theme::CORAL
                        } else if matches!(status, text_input::Status::Focused { .. }) {
                            theme::CYAN
                        } else {
                            theme::BORDER
                        },
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    icon: theme::TEXT_MUTED,
                    placeholder: theme::TEXT_MUTED,
                    value: theme::TEXT,
                    selection: theme::SELECTION,
                }),
            small("↑", Message::TermFindStep(true)),
            small("↓", Message::TermFindStep(false)),
            text(status).size(12).color(if not_found {
                theme::CORAL
            } else {
                theme::TEXT_MUTED
            }),
            space::horizontal(),
            small("× (Esc)", Message::TermFindClose),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    )
    .padding([6, 12])
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

/// "Programs are still running" confirmation for quit / close tab / close pane.
fn view_close_confirm<'a>(target: &CloseTarget, programs: &'a [String]) -> Element<'a, Message> {
    let (title, action) = match target {
        CloseTarget::Quit => ("Quit ForgeTerm?", "Quit (Enter)"),
        CloseTarget::Tab(_) => ("Close this tab?", "Close tab (Enter)"),
        CloseTarget::Pane(_) => ("Close this pane?", "Close pane (Enter)"),
    };
    let mut list = column![].spacing(4);
    for p in programs.iter().take(8) {
        list = list.push(text(format!("• {p}")).size(13).color(theme::TEXT));
    }
    if programs.len() > 8 {
        list = list.push(
            text(format!("… and {} more", programs.len() - 8))
                .size(12)
                .color(theme::TEXT_MUTED),
        );
    }
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
    let card = container(
        column![
            text(title).size(18).color(theme::TEXT),
            text(if programs.len() == 1 {
                "This program is still running and will be ended:"
            } else {
                "These programs are still running and will be ended:"
            })
            .size(13)
            .color(theme::TEXT_MUTED),
            list,
            row![
                space::horizontal(),
                button(text("Cancel (Esc)").size(13))
                    .on_press(Message::CancelClose)
                    .padding([6, 14])
                    .style(button_style(theme::CYAN)),
                button(text(action).size(13))
                    .on_press(Message::ConfirmClose)
                    .padding([6, 14])
                    .style(button_style(theme::CORAL)),
            ]
            .spacing(8),
        ]
        .spacing(12),
    )
    .padding(22)
    .width(Length::Fixed(480.0))
    .style(|_| container::Style {
        background: Some(theme::SURFACE.into()),
        border: Border {
            color: theme::AMBER,
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
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.45).into()),
            ..Default::default()
        });
    opaque(mouse_area(backdrop).on_press(Message::CancelClose))
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
            button(text(crate::commands::WEBSITE).size(13))
                .on_press(Message::Command(CommandId::Website))
                .padding(0)
                .style(|_, status| link_button(status)),
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
            // The worker thread panicked: reset the busy flags (and log it).
            Err(_) => Message::WorkerPanicked,
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

/// Keys of the focused viewer; `None` lets the key fall through to app shortcuts.
fn viewer_key(key: &keyboard::Key, mods: keyboard::Modifiers, v: &Viewer) -> Option<ViewerMsg> {
    let rows = v.tab().map_or(30, |t| t.rows) as i32;
    let ctrl = mods.control() && !mods.alt();
    Some(match key {
        keyboard::Key::Named(Named::ArrowUp) => ViewerMsg::Scroll(1),
        keyboard::Key::Named(Named::ArrowDown) => ViewerMsg::Scroll(-1),
        keyboard::Key::Named(Named::PageUp) => ViewerMsg::Scroll(rows - 1),
        keyboard::Key::Named(Named::PageDown) => ViewerMsg::Scroll(-(rows - 1)),
        keyboard::Key::Named(Named::Home) => ViewerMsg::Scroll(i32::MAX / 2),
        keyboard::Key::Named(Named::End) => ViewerMsg::Scroll(-(i32::MAX / 2)),
        keyboard::Key::Named(Named::F3) if mods.shift() => ViewerMsg::FindPrev,
        keyboard::Key::Named(Named::F3) => ViewerMsg::FindNext,
        keyboard::Key::Named(Named::Escape) => ViewerMsg::Escape,
        keyboard::Key::Character(c) if ctrl && !mods.shift() && c.eq_ignore_ascii_case("c") => {
            ViewerMsg::CopySelection
        }
        keyboard::Key::Character(c) if ctrl && !mods.shift() && c.eq_ignore_ascii_case("f") => {
            ViewerMsg::OpenFind
        }
        keyboard::Key::Character(c) if ctrl && !mods.shift() && c.eq_ignore_ascii_case("a") => {
            ViewerMsg::SelectAll
        }
        _ => return None,
    })
}

/// A text link: cyan, brighter on hover, no background.
fn link_button(status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: match status {
            button::Status::Hovered | button::Status::Pressed => theme::TEXT,
            _ => theme::CYAN,
        },
        ..Default::default()
    }
}

/// Opens a web link from terminal output in the default browser, only after a
/// Ctrl+click and only for http(s) links that pass `is_openable` (terminal output
/// is untrusted). The link is passed as one argument, never through a shell.
fn open_url(url: &str) -> Result<(), String> {
    if !forgeterm_terminal::is_openable(url) {
        return Err(format!("not opened: {url} is not a plain http(s) link"));
    }
    let program = if cfg!(windows) {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(url)
        .spawn()
        .map(drop)
        .map_err(|e| format!("could not open the link with {program}: {e}"))
}

/// Writes a rendered frame as PNG (test hook, see FORGETERM_SCREENSHOT).
fn save_screenshot(shot: &iced::window::Screenshot, path: &Path) {
    let size = resvg::tiny_skia::IntSize::from_wh(shot.size.width, shot.size.height);
    let png = size
        .and_then(|s| resvg::tiny_skia::Pixmap::from_vec(shot.rgba.to_vec(), s))
        .and_then(|p| p.encode_png().ok());
    match png.map(|bytes| std::fs::write(path, bytes)) {
        Some(Ok(())) => tracing::info!("screenshot saved to {}", path.display()),
        Some(Err(e)) => tracing::warn!("screenshot not saved: {e}"),
        None => tracing::warn!("screenshot not saved: could not encode the frame"),
    }
}

/// Looks up a Ctrl+clicked file reference off the UI thread (a stat can be slow):
/// relative paths against the terminal's folder, then its project root.
fn resolve_file_ref(wanted: String, line: Option<u32>, dir: Option<PathBuf>) -> Task<Message> {
    let key = wanted.clone();
    background(
        move || {
            let mut bases = Vec::new();
            if let Some(dir) = dir {
                let (root, _) = forgeterm_agents::project_root(&dir);
                bases.push(dir);
                if !bases.contains(&root) {
                    bases.push(root);
                }
            }
            forgeterm_terminal::resolve_file_ref(&key, &bases, Path::is_file)
        },
        move |found| Message::FileRefResolved {
            wanted: wanted.clone(),
            line,
            found,
        },
    )
}

fn load_document(path: PathBuf) -> Task<Message> {
    let key = path.clone();
    background(
        move || forgeterm_viewer::load(&path).map(Box::new),
        move |result| Message::Viewer(ViewerMsg::Loaded(key.clone(), result)),
    )
}

enum GitAction {
    Stage(Vec<String>),
    Unstage(Vec<String>),
    Commit(String),
    Switch(String),
    Track(String),
    Create(String),
}

fn describe(paths: &[String]) -> String {
    match paths {
        [one] => one.clone(),
        many => format!("{} files", many.len()),
    }
}

/// One refresh: find the repository (unless known) and read its status (and log).
fn git_snapshot(
    dir: PathBuf,
    known_root: Option<PathBuf>,
    with_log: bool,
    trusted: &[PathBuf],
) -> Snapshot {
    let root = match known_root.filter(|r| r.is_dir()) {
        Some(r) => Ok(Some(r)),
        None => forgeterm_git::discover(&dir),
    };
    let repo = match root {
        Err(e) => Repo::Error(e),
        Ok(None) => Repo::Outside,
        Ok(Some(root)) => {
            // Checked on every refresh: the repository's config can change.
            let programs = if trusted.contains(&root) {
                Ok(Vec::new())
            } else {
                forgeterm_git::repo_programs(&root)
            };
            match programs {
                Err(e) => Repo::Error(e),
                Ok(p) if !p.is_empty() => Repo::Untrusted { root, programs: p },
                Ok(_) => match forgeterm_git::status(&root) {
                    Ok(status) => Repo::Found { root, status },
                    Err(e) => Repo::Error(e),
                },
            }
        }
    };
    let (log, branches) = match (&repo, with_log) {
        (Repo::Found { root, .. }, true) => (
            Some(forgeterm_git::log(root, 15)),
            Some(forgeterm_git::branches(root)),
        ),
        _ => (None, None),
    };
    Snapshot {
        dir,
        repo,
        log,
        branches,
    }
}

/// Everything the Git Control Center shows besides the status (background thread).
/// Other worktrees are read like the main one: paused when their config names
/// programs and they are not trusted, and never when their folder is not local.
fn load_dashboard(root: PathBuf, trusted: &[PathBuf]) -> DashData {
    let root_key = git_dash::path_key(&root);
    let read = |i: usize, worktree: &forgeterm_git::Worktree| -> WorktreeChanges {
        let path = &worktree.path;
        if git_dash::path_key(path) == root_key {
            WorktreeChanges::Current
        } else if worktree.bare || i >= git_dash::MAX_WORKTREE_STATUS {
            WorktreeChanges::NotRead
        } else if !path.to_str().is_some_and(forgeterm_terminal::is_local_path)
            || !forgeterm_fs::resolves_locally(path)
        {
            WorktreeChanges::NotLocal
        } else if !path.is_dir() {
            WorktreeChanges::Missing
        } else {
            match forgeterm_git::repo_programs(path) {
                Err(e) => WorktreeChanges::Error(e),
                Ok(p) if !p.is_empty() && !trusted.contains(path) => WorktreeChanges::Paused,
                Ok(_) => match forgeterm_git::status(path) {
                    Ok(s) => WorktreeChanges::Count {
                        changes: s.change_count(),
                        conflicts: s.conflicted().count(),
                    },
                    Err(e) => WorktreeChanges::Error(e),
                },
            }
        }
    };
    // In parallel: one slow worktree must not hold up the others.
    let worktrees = forgeterm_git::worktrees(&root).map(|list| {
        let changes: Vec<WorktreeChanges> = std::thread::scope(|scope| {
            let handles: Vec<_> = list
                .iter()
                .enumerate()
                .map(|(i, w)| scope.spawn(move || read(i, w)))
                .collect();
            handles
                .into_iter()
                .map(|h| {
                    h.join().unwrap_or_else(|_| {
                        WorktreeChanges::Error("reading it failed unexpectedly".to_owned())
                    })
                })
                .collect()
        });
        list.into_iter()
            .zip(changes)
            .map(|(worktree, changes)| WorktreeInfo { worktree, changes })
            .collect()
    });
    DashData {
        unstaged: forgeterm_git::numstat(&root, false),
        staged: forgeterm_git::numstat(&root, true),
        activity: forgeterm_git::activity(&root, 30),
        commits: forgeterm_git::history(&root, 8),
        branches: forgeterm_git::branches(&root),
        worktrees,
        root,
        loaded_at: std::time::Instant::now(),
    }
}

fn load_prompts(dir: Option<PathBuf>) -> Task<Message> {
    background(
        move || forgeterm_prompts::load(dir.as_deref()),
        Message::PromptsLoaded,
    )
}

/// Example written when the prompts folder is created, so it is clear what goes in.
const EXAMPLE_PROMPT: &str = "---
name: Example (edit me)
description: Your own template. Every .md file in this folder appears in the Prompt Studio.
---
Explain {{file}} in {{project}} in simple terms. Focus on: {{focus}}
";

/// Creates the prompts folder (with an example on first use) and opens it in the
/// system file manager.
fn open_prompts_folder(dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        std::fs::write(dir.join("example.md"), EXAMPLE_PROMPT)
            .map_err(|e| format!("could not write the example template: {e}"))?;
    }
    let program = if cfg!(windows) {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(dir)
        .spawn()
        .map(drop)
        .map_err(|e| format!("could not open {} with {program}: {e}", dir.display()))
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

/// Takes the keyboard focus away from every text field (so keys reach the terminal):
/// iced's focus operation unfocuses every widget whose id differs from the target,
/// and no widget carries this id.
fn unfocus_widgets() -> Task<Message> {
    iced::widget::operation::focus(iced::widget::Id::new("forgeterm-no-widget"))
}
