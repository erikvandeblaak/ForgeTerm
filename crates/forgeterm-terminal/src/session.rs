use std::borrow::Cow;
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use alacritty_terminal::event::{Event as AlacEvent, EventListener, OnResize, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Boundary, Column, Direction, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::search::RegexSearch;
use alacritty_terminal::term::{self, Term, TermMode, viewport_to_point};
use alacritty_terminal::tty::{self, ChildEvent, EventedPty, EventedReadWrite};
use alacritty_terminal::vte::ansi::CursorShape as AlacCursorShape;
use anyhow::Context;

use crate::cwd::CwdScanner;
use crate::palette::Palette;
use crate::shell::ShellSpec;
use crate::snapshot::{CellFlags, CellView, CursorShape, CursorView, Snapshot};

/// Terminal size in cells plus the pixel size of one cell (reported to the PTY).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl GridSize {
    pub fn new(cols: u16, rows: u16, cell_width: u16, cell_height: u16) -> Self {
        Self {
            cols: cols.max(2),
            rows: rows.max(1),
            cell_width,
            cell_height,
        }
    }

    fn window_size(self) -> WindowSize {
        WindowSize {
            num_cols: self.cols,
            num_lines: self.rows,
            cell_width: self.cell_width,
            cell_height: self.cell_height,
        }
    }
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        usize::from(self.rows)
    }

    fn screen_lines(&self) -> usize {
        usize::from(self.rows)
    }

    fn columns(&self) -> usize {
        usize::from(self.cols)
    }
}

/// How to start a session.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// `None` uses [`crate::default_shell`].
    pub shell: Option<ShellSpec>,
    pub working_directory: Option<PathBuf>,
    pub env: HashMap<String, String>,
    pub size: GridSize,
    pub scrollback: usize,
    pub palette: Palette,
    /// Let the shell report its working directory (see [`crate::integration`]).
    pub shell_integration: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            shell: None,
            working_directory: None,
            env: HashMap::new(),
            size: GridSize::new(80, 24, 8, 16),
            scrollback: 10_000,
            palette: Palette::default(),
            shell_integration: true,
        }
    }
}

/// Events a session reports to its owner (UI). Sent from the I/O thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    /// New content is available; take a [`Session::snapshot`]. Coalesced: at most one
    /// pending wakeup until the next snapshot is taken.
    Wakeup,
    Title(String),
    ResetTitle,
    Bell,
    /// The program asked to put text on the clipboard (OSC 52).
    ClipboardStore(String),
    /// The child process exited (exit code if known).
    Exited(Option<i32>),
    /// The shell reported its working directory (OSC 7 / OSC 9;9); only sent for
    /// existing directories. A hint printed by whatever runs in the terminal: use it
    /// to pick directories, never execute it.
    Cwd(String),
}

/// Terminal modes relevant to input handling.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modes {
    pub app_cursor: bool,
    pub bracketed_paste: bool,
    pub mouse_reporting: bool,
    pub mouse_motion: bool,
    pub sgr_mouse: bool,
    pub alt_screen: bool,
    pub alternate_scroll: bool,
    /// Kitty keyboard protocol flags the program enabled (0 = legacy encoding):
    /// 1 disambiguate, 2 event types, 4 alternate keys, 8 all keys as escape
    /// codes, 16 associated text.
    pub keyboard: u8,
}

type Sink = Box<dyn Fn(SessionEvent) + Send + Sync>;

struct ListenerInner {
    sender: OnceLock<EventLoopSender>,
    size: Mutex<GridSize>,
    palette: Palette,
    wakeup_pending: AtomicBool,
    sink: Sink,
}

#[derive(Clone)]
pub struct Listener(Arc<ListenerInner>);

impl Listener {
    fn write(&self, bytes: Vec<u8>) {
        if let Some(sender) = self.0.sender.get() {
            let _ = sender.send(Msg::Input(Cow::Owned(bytes)));
        }
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: AlacEvent) {
        let inner = &self.0;
        match event {
            AlacEvent::Wakeup => {
                if !inner.wakeup_pending.swap(true, Ordering::AcqRel) {
                    (inner.sink)(SessionEvent::Wakeup);
                }
            }
            // Replies the terminal must send back to the program (DA, DSR, ...).
            AlacEvent::PtyWrite(text) => self.write(text.into_bytes()),
            AlacEvent::ColorRequest(index, format) => {
                let c = inner.palette.by_index(index);
                let rgb = alacritty_terminal::vte::ansi::Rgb {
                    r: c.r,
                    g: c.g,
                    b: c.b,
                };
                self.write(format(rgb).into_bytes());
            }
            AlacEvent::TextAreaSizeRequest(format) => {
                let size = *inner.size.lock().unwrap_or_else(|e| e.into_inner());
                self.write(format(size.window_size()).into_bytes());
            }
            AlacEvent::Title(title) => (inner.sink)(SessionEvent::Title(title)),
            AlacEvent::ResetTitle => (inner.sink)(SessionEvent::ResetTitle),
            AlacEvent::Bell => (inner.sink)(SessionEvent::Bell),
            AlacEvent::ClipboardStore(_, text) => (inner.sink)(SessionEvent::ClipboardStore(text)),
            AlacEvent::ChildExit(status) => (inner.sink)(SessionEvent::Exited(status.code())),
            // Clipboard *reads* by programs (OSC 52 query) are deliberately not served:
            // terminal output must never be able to exfiltrate the user's clipboard.
            AlacEvent::ClipboardLoad(..)
            | AlacEvent::Exit
            | AlacEvent::MouseCursorDirty
            | AlacEvent::CursorBlinkingChange => {}
        }
    }
}

/// The PTY with a tap on its output: everything is passed through unchanged to the
/// emulator, while [`CwdScanner`] looks for working-directory reports.
struct ScanPty<P> {
    inner: P,
    scanner: CwdScanner,
    last: Option<String>,
    listener: Listener,
}

impl<P: EventedReadWrite> io::Read for ScanPty<P> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.reader().read(buf)?;
        // Checked here, on the I/O thread, so a slow (network) path never stalls the UI.
        if let Some(cwd) = self.scanner.feed(&buf[..n])
            && self.last.as_ref() != Some(&cwd)
            && std::path::Path::new(&cwd).is_dir()
        {
            self.last = Some(cwd.clone());
            (self.listener.0.sink)(SessionEvent::Cwd(cwd));
        }
        Ok(n)
    }
}

impl<P: EventedReadWrite> EventedReadWrite for ScanPty<P> {
    type Reader = Self;
    type Writer = P::Writer;

    // Pure delegation: the trait method is `unsafe` because the caller must keep the
    // sources alive while registered. That contract is passed on unchanged to the
    // wrapped PTY, which owns the sources; this wrapper adds no unsafe operation.
    #[allow(unsafe_code)]
    unsafe fn register(
        &mut self,
        poller: &Arc<polling::Poller>,
        interest: polling::Event,
        mode: polling::PollMode,
    ) -> io::Result<()> {
        // SAFETY: same contract as this method; the caller upholds it.
        unsafe { self.inner.register(poller, interest, mode) }
    }

    fn reregister(
        &mut self,
        poller: &Arc<polling::Poller>,
        interest: polling::Event,
        mode: polling::PollMode,
    ) -> io::Result<()> {
        self.inner.reregister(poller, interest, mode)
    }

    fn deregister(&mut self, poller: &Arc<polling::Poller>) -> io::Result<()> {
        self.inner.deregister(poller)
    }

    fn reader(&mut self) -> &mut Self {
        self
    }

    fn writer(&mut self) -> &mut P::Writer {
        self.inner.writer()
    }
}

impl<P: EventedPty> EventedPty for ScanPty<P> {
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        self.inner.next_child_event()
    }
}

impl<P: OnResize> OnResize for ScanPty<P> {
    fn on_resize(&mut self, window_size: WindowSize) {
        self.inner.on_resize(window_size);
    }
}

/// Result of [`Session::search`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchOutcome {
    Found,
    NotFound,
}

/// One live shell: PTY + child process + emulator state.
pub struct Session {
    term: Arc<FairMutex<Term<Listener>>>,
    sender: EventLoopSender,
    listener: Listener,
    size: GridSize,
    shell: ShellSpec,
    /// Process id of the shell (to see what runs inside it).
    pid: Option<u32>,
}

impl Session {
    /// Spawns the shell and starts the I/O thread. `sink` is called from that thread.
    pub fn spawn(
        config: SessionConfig,
        sink: impl Fn(SessionEvent) + Send + Sync + 'static,
    ) -> anyhow::Result<Self> {
        let shell = match config.shell {
            Some(s) => s,
            None => crate::shell::default_shell().context("no supported shell found on PATH")?,
        };

        let mut env = config.env;
        env.entry("TERM".into())
            .or_insert_with(|| "xterm-256color".into());
        env.entry("COLORTERM".into())
            .or_insert_with(|| "truecolor".into());
        env.insert("TERM_PROGRAM".into(), "ForgeTerm".into());
        env.insert(
            "TERM_PROGRAM_VERSION".into(),
            env!("CARGO_PKG_VERSION").into(),
        );

        let mut args = shell.args.clone();
        if config.shell_integration {
            let extra = crate::integration::integration(&shell, |key| {
                env.get(key).cloned().or_else(|| std::env::var(key).ok())
            });
            args.extend(extra.args);
            env.extend(extra.env);
        }

        let options = tty::Options {
            shell: Some(tty::Shell::new(shell.program.clone(), args)),
            working_directory: config.working_directory,
            drain_on_exit: true,
            env,
            #[cfg(windows)]
            escape_args: true,
        };

        let size = config.size;
        let listener = Listener(Arc::new(ListenerInner {
            sender: OnceLock::new(),
            size: Mutex::new(size),
            palette: config.palette,
            wakeup_pending: AtomicBool::new(false),
            sink: Box::new(sink),
        }));

        let pty = tty::new(&options, size.window_size(), 0)
            .with_context(|| format!("failed to start shell '{}'", shell.program))?;
        #[cfg(windows)]
        let pid = pty.child_watcher().pid().map(std::num::NonZeroU32::get);
        #[cfg(unix)]
        let pid = Some(pty.child().id());
        let pty = ScanPty {
            inner: pty,
            scanner: CwdScanner::default(),
            last: None,
            listener: listener.clone(),
        };

        let term_config = term::Config {
            scrolling_history: config.scrollback,
            kitty_keyboard: true,
            ..Default::default()
        };
        let term = Arc::new(FairMutex::new(Term::new(
            term_config,
            &size,
            listener.clone(),
        )));

        let event_loop = EventLoop::new(term.clone(), listener.clone(), pty, true, false)
            .context("failed to create PTY event loop")?;
        let sender = event_loop.channel();
        let _ = listener.0.sender.set(sender.clone());
        // The I/O thread owns the PTY; it ends when the child exits or on Shutdown.
        drop(event_loop.spawn());

        tracing::info!(shell = %shell.program, cols = size.cols, rows = size.rows, "session started");
        Ok(Self {
            term,
            sender,
            listener,
            size,
            shell,
            pid,
        })
    }

    /// Process id of the shell, if the platform reports it.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn shell(&self) -> &ShellSpec {
        &self.shell
    }

    pub fn size(&self) -> GridSize {
        self.size
    }

    /// Raw bytes to the PTY.
    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        let _ = self.sender.send(Msg::Input(bytes.into()));
    }

    /// Typed input: jumps back to the live view, clears the selection and writes.
    pub fn input(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        {
            let mut term = self.term.lock();
            term.scroll_display(Scroll::Bottom);
            term.selection = None;
        }
        self.write(bytes);
    }

    /// Pastes text (see [`crate::encode_paste`]). Callers should check
    /// [`crate::paste_needs_confirmation`] first and ask the user.
    pub fn paste(&self, text: &str) {
        let bytes = crate::input::encode_paste(text, self.modes().bracketed_paste);
        if !bytes.is_empty() {
            self.input(bytes);
        }
    }

    pub fn resize(&mut self, size: GridSize) {
        if size == self.size {
            return;
        }
        self.size = size;
        *self
            .listener
            .0
            .size
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = size;
        self.term.lock().resize(size);
        let _ = self.sender.send(Msg::Resize(size.window_size()));
    }

    /// Scrolls the viewport through history; positive = up (older output).
    pub fn scroll(&self, lines: i32) {
        self.term.lock().scroll_display(Scroll::Delta(lines));
    }

    pub fn scroll_to_top(&self) {
        self.term.lock().scroll_display(Scroll::Top);
    }

    pub fn scroll_to_bottom(&self) {
        self.term.lock().scroll_display(Scroll::Bottom);
    }

    /// Drops all scrollback history (the visible screen is kept). Clears the
    /// selection too, since it may point into the removed lines.
    pub fn clear_scrollback(&self) {
        let mut term = self.term.lock();
        term.selection = None;
        term.grid_mut().clear_history();
    }

    pub fn modes(&self) -> Modes {
        let term = self.term.lock();
        let m = *term.mode();
        Modes {
            app_cursor: m.contains(TermMode::APP_CURSOR),
            bracketed_paste: m.contains(TermMode::BRACKETED_PASTE),
            mouse_reporting: m.intersects(TermMode::MOUSE_MODE),
            mouse_motion: m.intersects(TermMode::MOUSE_MOTION | TermMode::MOUSE_DRAG),
            sgr_mouse: m.contains(TermMode::SGR_MOUSE),
            alt_screen: m.contains(TermMode::ALT_SCREEN),
            alternate_scroll: m.contains(TermMode::ALTERNATE_SCROLL),
            keyboard: ((m & TermMode::KITTY_KEYBOARD_PROTOCOL).bits() >> 18) as u8,
        }
    }

    fn viewport_point(term: &Term<Listener>, col: usize, row: usize) -> Point {
        let offset = term.grid().display_offset();
        let col = col.min(term.columns().saturating_sub(1));
        let row = row.min(term.screen_lines().saturating_sub(1));
        viewport_to_point(offset, Point::new(row, Column(col)))
    }

    /// Starts a text selection at a viewport cell. `right_half` = pointer is on the
    /// right half of the cell.
    pub fn start_selection(&self, col: usize, row: usize, right_half: bool) {
        let mut term = self.term.lock();
        let point = Self::viewport_point(&term, col, row);
        let side = if right_half { Side::Right } else { Side::Left };
        term.selection = Some(Selection::new(SelectionType::Simple, point, side));
    }

    pub fn update_selection(&self, col: usize, row: usize, right_half: bool) {
        let mut term = self.term.lock();
        let point = Self::viewport_point(&term, col, row);
        let side = if right_half { Side::Right } else { Side::Left };
        if let Some(sel) = term.selection.as_mut() {
            sel.update(point, side);
        }
    }

    pub fn clear_selection(&self) {
        self.term.lock().selection = None;
    }

    /// Finds `query` as literal text (smart case: case-insensitive unless it
    /// contains capitals) in the screen and scrollback, starting next to the
    /// current selection (the previous match) or at the cursor. `older` searches
    /// upwards. The match becomes the selection (so Ctrl+C copies it) and the view
    /// scrolls to it. Wraps around at the ends.
    pub fn search(&self, query: &str, older: bool) -> SearchOutcome {
        if query.is_empty() {
            return SearchOutcome::NotFound;
        }
        let Ok(mut regex) = RegexSearch::new(&crate::search::escape(query)) else {
            return SearchOutcome::NotFound;
        };
        let mut term = self.term.lock();
        let origin = match term.selection.as_ref().and_then(|s| s.to_range(&*term)) {
            // Step off the previous match so the next one is found.
            Some(range) if older => range.start.sub(&*term, Boundary::None, 1),
            Some(range) => range.end.add(&*term, Boundary::None, 1),
            None => term.grid().cursor.point,
        };
        let direction = if older {
            Direction::Left
        } else {
            Direction::Right
        };
        let Some(found) = term.search_next(&mut regex, origin, direction, Side::Left, None) else {
            return SearchOutcome::NotFound;
        };
        let mut selection = Selection::new(SelectionType::Simple, *found.start(), Side::Left);
        selection.update(*found.end(), Side::Right);
        term.selection = Some(selection);
        term.scroll_to_point(*found.start());
        SearchOutcome::Found
    }

    pub fn selection_text(&self) -> Option<String> {
        self.term
            .lock()
            .selection_to_string()
            .filter(|s| !s.is_empty())
    }

    /// Copies the visible viewport into a renderer-friendly snapshot.
    pub fn snapshot(&self, palette: &Palette) -> Snapshot {
        // Clear before locking so output arriving during the copy re-notifies.
        self.listener
            .0
            .wakeup_pending
            .store(false, Ordering::Release);

        let term = self.term.lock();
        let content = term.renderable_content();
        let cols = term.columns();
        let rows = term.screen_lines();
        let offset = content.display_offset;
        let colors = content.colors;
        let selection = content.selection;

        let blank = CellView {
            c: ' ',
            zerowidth: None,
            fg: palette.foreground,
            bg: palette.background,
            flags: CellFlags::default(),
        };
        let mut cells = vec![blank; cols * rows];

        for indexed in content.display_iter {
            let point = indexed.point;
            let row = point.line.0 + offset as i32;
            if row < 0 || row as usize >= rows || point.column.0 >= cols {
                continue;
            }
            let cell = indexed.cell;
            let f = cell.flags;

            let mut fg = palette.resolve(cell.fg, colors);
            let mut bg = palette.resolve(cell.bg, colors);
            if f.contains(Flags::DIM) {
                fg = fg.dim();
            }
            if f.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if f.contains(Flags::HIDDEN) {
                fg = bg;
            }
            if selection.is_some_and(|s| s.contains(point)) {
                fg = palette.selection_fg;
                bg = palette.selection_bg;
            }

            cells[row as usize * cols + point.column.0] = CellView {
                c: cell.c,
                zerowidth: cell
                    .zerowidth()
                    .map(|z| z.iter().collect::<String>().into_boxed_str()),
                fg,
                bg,
                flags: CellFlags {
                    bold: f.contains(Flags::BOLD),
                    italic: f.contains(Flags::ITALIC),
                    underline: f.intersects(Flags::ALL_UNDERLINES),
                    strikeout: f.contains(Flags::STRIKEOUT),
                    wide: f.contains(Flags::WIDE_CHAR),
                    wide_spacer: f.contains(Flags::WIDE_CHAR_SPACER),
                },
            };
        }

        let cursor = {
            let c = content.cursor;
            let row = c.point.line.0 + offset as i32;
            let shape = match c.shape {
                AlacCursorShape::Block => Some(CursorShape::Block),
                AlacCursorShape::Underline => Some(CursorShape::Underline),
                AlacCursorShape::Beam => Some(CursorShape::Beam),
                AlacCursorShape::HollowBlock => Some(CursorShape::HollowBlock),
                AlacCursorShape::Hidden => None,
            };
            shape
                .filter(|_| row >= 0 && (row as usize) < rows)
                .map(|shape| CursorView {
                    col: c.point.column.0.min(cols.saturating_sub(1)),
                    row: row as usize,
                    shape,
                    color: palette.cursor,
                })
        };

        Snapshot {
            cols,
            rows,
            cells,
            cursor,
            background: palette.background,
            display_offset: offset,
            history: term.grid().history_size(),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.sender.send(Msg::Shutdown);
    }
}
