//! Application state, update loop and layout: tabs of split terminal panes.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use forgeterm_agents::{Agent, AgentKind, ProjectContext};
use forgeterm_explorer::{Listing, Tree, list_dir};
use forgeterm_settings::{Settings, ThemeChoice};
use forgeterm_terminal::{
    Palette, SessionEvent, ShellKind, ShellSpec, detect_shells, paste_needs_confirmation,
    quote_path,
};
use iced::keyboard::{self, key::Named};
use iced::widget::canvas::Canvas;
use iced::widget::pane_grid::{self, Axis, Direction, PaneGrid};
use iced::widget::{
    button, container, mouse_area, opaque, pick_list, row, scrollable, space, stack, svg, text,
    text_input,
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

const LOGO: &[u8] = include_bytes!("../../../../assets/branding/forgeterm-logo.svg");
/// Start of the notice when git refuses to remove a worktree.
const REMOVE_FAILED: &str = "git did not remove the worktree: ";
const MAX_TAB_TITLE: usize = 28;
/// The horizontally scrolling tab strip.
const TABS_ID: &str = "forgeterm-tabs";
pub const EXPLORER_LIST_ID: &str = "forgeterm-explorer-list";
pub const EXPLORER_GOTO_ID: &str = "forgeterm-explorer-goto";

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
/// An entry of the "New tab with…" picker: a shell or an SSH host.
pub struct ShellChoice {
    command: CommandId,
    name: String,
}

impl fmt::Display for ShellChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

pub struct App {
    /// The OpenSSH client, if installed.
    ssh_program: Option<PathBuf>,
    /// `Host` entries of `~/.ssh/config` (read only).
    ssh_hosts: Vec<forgeterm_terminal::ssh::SshHost>,
    ssh_config_mtime: Option<SystemTime>,
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

mod ai;
mod close;
mod compare;
mod git;
mod handle;
mod input;
mod mission;
mod monitor;
mod panels;
mod run;
mod session;
mod settings;
mod tasks;
mod terminals;
mod view;
mod viewer;
mod widgets;

use tasks::*;
use widgets::*;

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let mut app = Self {
            terminals: HashMap::new(),
            workspace: Workspace::default(),
            next_id: 1,
            shells: detect_shells(),
            palette: theme::terminal_palette(),
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
            ssh_program: forgeterm_terminal::ssh::find_ssh(),
            ssh_hosts: Vec::new(),
            ssh_config_mtime: None,
        };
        app.load_ssh_hosts();
        app.load_settings(true);
        let terminal = app.restore_or_new_tab();
        let root = app.explorer.root().to_path_buf();
        (
            app,
            Task::batch([terminal, load_dir(root), detect_agents()]),
        )
    }

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
}
