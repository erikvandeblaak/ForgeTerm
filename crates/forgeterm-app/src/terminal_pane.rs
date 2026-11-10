//! One terminal inside a tab: its session, render state and lifecycle.

use std::path::PathBuf;

use forgeterm_terminal::{
    GridSize, Modes, Palette, Session, SessionConfig, SessionEvent, ShellSpec, Snapshot,
};
use iced::Task;
use iced::futures::channel::mpsc;
use iced::widget::canvas::Cache;

use crate::app::Message;
use crate::terminal_view::Metrics;

/// Stable identity of a terminal for its whole life (survives moves between panes).
pub type TermId = u64;

/// Messages produced by a terminal's canvas.
#[derive(Debug, Clone)]
pub enum TermMsg {
    GridResized(u16, u16),
    Scroll(i32),
    SelectStart(usize, usize, bool),
    SelectUpdate(usize, usize, bool),
    SelectEnd,
    /// Bytes for the PTY (mouse reports, wheel-as-arrows). `focus`: came from a click.
    Input {
        bytes: Vec<u8>,
        focus: bool,
    },
    Paste,
}

impl TermMsg {
    /// Whether this interaction should move keyboard focus to the terminal.
    pub fn focuses(&self) -> bool {
        matches!(
            self,
            TermMsg::SelectStart(..) | TermMsg::Paste | TermMsg::Input { focus: true, .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Running,
    Exited(Option<i32>),
    Failed(String),
}

/// What the app must do after a session event.
pub enum Outcome {
    None,
    /// The shell ended cleanly; the pane should close.
    Close,
    Task(Task<Message>),
}

pub struct TerminalPane {
    pub id: TermId,
    pub shell: ShellSpec,
    working_directory: Option<PathBuf>,
    session: Option<Session>,
    /// Incremented on every (re)start so events of a previous session are ignored.
    generation: u64,
    pub status: Status,
    pub snapshot: Snapshot,
    pub modes: Modes,
    pub cache: Cache,
    pub grid: (u16, u16),
    pub title: Option<String>,
    /// New output arrived while the terminal was not visible.
    dirty: bool,
}

impl TerminalPane {
    pub fn new(
        id: TermId,
        shell: ShellSpec,
        working_directory: Option<PathBuf>,
        metrics: Metrics,
        palette: &Palette,
    ) -> (Self, Task<Message>) {
        let mut pane = Self {
            id,
            shell,
            working_directory,
            session: None,
            generation: 0,
            status: Status::Running,
            snapshot: blank_snapshot(palette, 80, 24),
            modes: Modes::default(),
            cache: Cache::new(),
            grid: (80, 24),
            title: None,
            dirty: false,
        };
        let task = pane.start(metrics, palette);
        (pane, task)
    }

    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    pub fn is_running(&self) -> bool {
        self.session.is_some()
    }

    fn grid_size(&self, metrics: Metrics) -> GridSize {
        GridSize::new(
            self.grid.0,
            self.grid.1,
            metrics.cell_width.round() as u16,
            metrics.cell_height.round() as u16,
        )
    }

    /// (Re)starts the shell and returns the task that streams its events.
    pub fn start(&mut self, metrics: Metrics, palette: &Palette) -> Task<Message> {
        self.generation += 1;
        let (id, generation) = (self.id, self.generation);
        let (tx, rx) = mpsc::unbounded();
        let config = SessionConfig {
            shell: Some(self.shell.clone()),
            working_directory: self.working_directory.clone(),
            size: self.grid_size(metrics),
            palette: palette.clone(),
            ..Default::default()
        };
        match Session::spawn(config, move |e| {
            let _ = tx.unbounded_send(e);
        }) {
            Ok(session) => {
                self.session = Some(session);
                self.status = Status::Running;
                self.title = None;
                self.refresh(palette);
                Task::run(rx, move |event| Message::Session {
                    id,
                    generation,
                    event,
                })
            }
            Err(err) => {
                tracing::error!(shell = %self.shell.program, "failed to start session: {err:#}");
                self.status = Status::Failed(format!("{err:#}"));
                Task::none()
            }
        }
    }

    /// Takes a fresh snapshot (call only when the terminal is visible).
    pub fn refresh(&mut self, palette: &Palette) {
        if let Some(session) = &self.session {
            self.snapshot = session.snapshot(palette);
            self.modes = session.modes();
            self.cache.clear();
        }
        self.dirty = false;
    }

    pub fn refresh_if_dirty(&mut self, palette: &Palette) {
        if self.dirty {
            self.refresh(palette);
        }
    }

    pub fn on_session_event(
        &mut self,
        generation: u64,
        event: SessionEvent,
        visible: bool,
        palette: &Palette,
    ) -> Outcome {
        if generation != self.generation {
            return Outcome::None;
        }
        match event {
            SessionEvent::Wakeup => {
                if visible {
                    self.refresh(palette);
                } else {
                    self.dirty = true;
                }
            }
            SessionEvent::Title(t) => self.title = Some(t),
            SessionEvent::ResetTitle => self.title = None,
            SessionEvent::Bell => {}
            SessionEvent::ClipboardStore(t) => {
                return Outcome::Task(iced::clipboard::write(t));
            }
            SessionEvent::Exited(code) => {
                self.refresh(palette);
                self.session = None;
                self.status = Status::Exited(code);
                // Like most terminals: a clean exit closes the pane, a failure stays
                // visible so the user can read what happened.
                if code == Some(0) {
                    return Outcome::Close;
                }
            }
        }
        Outcome::None
    }

    pub fn resize(&mut self, cols: u16, rows: u16, metrics: Metrics, palette: &Palette) {
        self.grid = (cols, rows);
        let size = self.grid_size(metrics);
        if let Some(session) = &mut self.session {
            session.resize(size);
        }
        self.refresh(palette);
    }

    /// Text for tabs and the window title.
    pub fn display_title(&self) -> String {
        display_title(self.title.as_deref(), &self.shell)
    }
}

/// Program-set title, unless it is empty or just the path of an executable (ConPTY
/// sets the title to e.g. `C:\Program Files\PowerShell\7\pwsh.exe`) — then the shell's
/// friendly name.
fn display_title(title: Option<&str>, shell: &ShellSpec) -> String {
    let is_exe = |s: &str| {
        s.eq_ignore_ascii_case(&shell.program)
            || std::path::Path::new(s)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
    };
    let Some(title) = title.map(str::trim).filter(|t| !t.is_empty()) else {
        return shell.name.clone();
    };
    if is_exe(title) {
        return shell.name.clone();
    }
    // cmd.exe: "C:\Windows\system32\cmd.exe - ping -n 5 127.0.0.1" → "ping -n 5 127.0.0.1".
    if let Some((head, rest)) = title.split_once(" - ")
        && is_exe(head)
        && !rest.trim().is_empty()
    {
        return rest.trim().to_owned();
    }
    title.to_owned()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pwsh() -> ShellSpec {
        ShellSpec::new(
            "PowerShell 7",
            r"C:\Program Files\PowerShell\7\pwsh.exe",
            vec![],
        )
    }

    #[test]
    fn executable_paths_become_the_shell_name() {
        let shell = pwsh();
        assert_eq!(display_title(None, &shell), "PowerShell 7");
        assert_eq!(display_title(Some("  "), &shell), "PowerShell 7");
        assert_eq!(
            display_title(Some(r"C:\Program Files\PowerShell\7\pwsh.exe"), &shell),
            "PowerShell 7"
        );
        assert_eq!(
            display_title(Some(r"C:\WINDOWS\system32\cmd.exe"), &shell),
            "PowerShell 7"
        );
    }

    #[test]
    fn cmd_running_command_shows_only_the_command() {
        assert_eq!(
            display_title(
                Some(r"C:\Windows\system32\cmd.exe - ping -n 5 127.0.0.1"),
                &pwsh()
            ),
            "ping -n 5 127.0.0.1"
        );
        // A " - " in a normal title is left alone.
        assert_eq!(
            display_title(Some("vim - notes.md"), &pwsh()),
            "vim - notes.md"
        );
    }

    #[test]
    fn program_titles_are_kept() {
        let shell = pwsh();
        assert_eq!(display_title(Some("claude"), &shell), "claude");
        assert_eq!(
            display_title(Some("vim Cargo.toml"), &shell),
            "vim Cargo.toml"
        );
    }
}
