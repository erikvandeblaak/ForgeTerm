//! Central command registry: every user-facing action has one `CommandId`, a title, a
//! category and zero or more key bindings. The Command Palette, keyboard handling and
//! the menu bar all read from here, so an action can never exist in one place
//! and be missing or behave differently in another.
//!
//! Pure data and logic — no iced types — so it is unit-tested.

use std::fmt;

use forgeterm_agents::AgentKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandId {
    CommandPalette,
    NewTab,
    /// Index into the detected shell list.
    NewTabWithShell(usize),
    CloseTab,
    ClosePane,
    NextTab,
    PreviousTab,
    /// Zero-based tab index.
    SelectTab(usize),
    SplitRight,
    SplitDown,
    TogglePaneZoom,
    FocusPane(Dir),
    RestartShell,
    ClearScrollback,
    /// Shell integration: scroll to the previous / next command's prompt.
    PreviousCommand,
    NextCommand,
    CopyLastCommandOutput,
    /// The focused shell's last command output into the agent's prompt (no Enter).
    InsertLastOutputIntoAgent,
    ScrollToTop,
    ScrollToBottom,
    ScrollPageUp,
    ScrollPageDown,
    Copy,
    Paste,
    FindInTerminal,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ToggleExplorer,
    ToggleViewer,
    ViewSelectedFile,
    ToggleMonitor,
    KeyboardShortcuts,
    ExplorerToggleHidden,
    ExplorerToggleFollow,
    ExplorerRefresh,
    ExplorerParent,
    ExplorerOpenSelected,
    /// Type or paste a folder for the explorer.
    ExplorerGoToFolder,
    InsertSelectedPath,
    CopySelectedPath,
    NewTabInSelectedFolder,
    OpenSettings,
    ReloadSettings,
    ToggleAiPanel,
    AiRefresh,
    /// Start an agent in the project root (`true` = in a split).
    StartAgent(AgentKind, bool),
    OpenPromptStudio,
    /// Prompt Studio as a panel beside the terminal, or as a floating window.
    TogglePromptStudioDock,
    OpenPromptsFolder,
    ToggleGitPanel,
    GitRefresh,
    GitShowHistory,
    About,
    /// Opens the maker's website ([`WEBSITE`]) in the default browser.
    Website,
    Quit,
}

/// The maker's website, shown in Help → About.
pub const WEBSITE: &str = "https://vandeblaak.com";

/// Commands that exist regardless of runtime state, in palette order.
/// Parameterised commands (shells, tabs) are added by the app from live data.
pub const STATIC_COMMANDS: &[CommandId] = &[
    CommandId::NewTab,
    CommandId::SplitRight,
    CommandId::SplitDown,
    CommandId::ClosePane,
    CommandId::CloseTab,
    CommandId::NextTab,
    CommandId::PreviousTab,
    CommandId::TogglePaneZoom,
    CommandId::FocusPane(Dir::Left),
    CommandId::FocusPane(Dir::Right),
    CommandId::FocusPane(Dir::Up),
    CommandId::FocusPane(Dir::Down),
    CommandId::RestartShell,
    CommandId::ClearScrollback,
    CommandId::PreviousCommand,
    CommandId::NextCommand,
    CommandId::CopyLastCommandOutput,
    CommandId::InsertLastOutputIntoAgent,
    CommandId::ScrollToTop,
    CommandId::ScrollToBottom,
    CommandId::ScrollPageUp,
    CommandId::ScrollPageDown,
    CommandId::Copy,
    CommandId::Paste,
    CommandId::FindInTerminal,
    CommandId::ZoomIn,
    CommandId::ZoomOut,
    CommandId::ZoomReset,
    CommandId::ToggleExplorer,
    CommandId::ToggleViewer,
    CommandId::ViewSelectedFile,
    CommandId::ToggleMonitor,
    CommandId::KeyboardShortcuts,
    CommandId::ExplorerToggleHidden,
    CommandId::ExplorerToggleFollow,
    CommandId::ExplorerRefresh,
    CommandId::ExplorerParent,
    CommandId::ExplorerOpenSelected,
    CommandId::ExplorerGoToFolder,
    CommandId::InsertSelectedPath,
    CommandId::CopySelectedPath,
    CommandId::NewTabInSelectedFolder,
    CommandId::OpenSettings,
    CommandId::ReloadSettings,
    CommandId::ToggleAiPanel,
    CommandId::AiRefresh,
    CommandId::OpenPromptStudio,
    CommandId::TogglePromptStudioDock,
    CommandId::OpenPromptsFolder,
    CommandId::ToggleGitPanel,
    CommandId::GitRefresh,
    CommandId::GitShowHistory,
    CommandId::About,
    CommandId::Website,
    CommandId::Quit,
];

impl CommandId {
    pub fn category(self) -> &'static str {
        use CommandId::*;
        match self {
            CommandPalette | ZoomIn | ZoomOut | ZoomReset | ToggleMonitor => "View",
            NewTab | NewTabWithShell(_) | CloseTab | NextTab | PreviousTab | SelectTab(_) => "Tab",
            SplitRight | SplitDown | ClosePane | TogglePaneZoom | FocusPane(_) => "Pane",
            RestartShell
            | ClearScrollback
            | PreviousCommand
            | NextCommand
            | CopyLastCommandOutput
            | ScrollToTop
            | ScrollToBottom
            | ScrollPageUp
            | ScrollPageDown => "Terminal",
            Copy | Paste | FindInTerminal => "Edit",
            ToggleExplorer
            | ExplorerToggleHidden
            | ExplorerToggleFollow
            | ExplorerRefresh
            | ExplorerParent
            | ExplorerOpenSelected
            | ExplorerGoToFolder
            | InsertSelectedPath
            | CopySelectedPath
            | NewTabInSelectedFolder => "Explorer",
            OpenSettings | ReloadSettings => "Settings",
            ToggleViewer | ViewSelectedFile => "Viewer",
            ToggleAiPanel
            | AiRefresh
            | StartAgent(..)
            | InsertLastOutputIntoAgent
            | OpenPromptStudio
            | TogglePromptStudioDock
            | OpenPromptsFolder => "AI",
            ToggleGitPanel | GitRefresh | GitShowHistory => "Git",
            About | Website | KeyboardShortcuts => "Help",
            Quit => "App",
        }
    }

    /// Stable name for settings.toml `[keys]` (and shown in Help → Keyboard
    /// Shortcuts). Parameterised commands carry their number / agent in the name.
    pub fn config_name(self) -> String {
        use CommandId::*;
        let fixed = match self {
            NewTabWithShell(i) => return format!("new_tab_with_shell_{}", i + 1),
            SelectTab(i) => return format!("select_tab_{}", i + 1),
            FocusPane(d) => {
                return format!(
                    "focus_pane_{}",
                    match d {
                        Dir::Left => "left",
                        Dir::Right => "right",
                        Dir::Up => "up",
                        Dir::Down => "down",
                    }
                );
            }
            StartAgent(kind, split) => {
                let agent = match kind {
                    AgentKind::ClaudeCode => "claude_code",
                    AgentKind::Codex => "codex",
                };
                return format!("start_{agent}{}", if split { "_split" } else { "" });
            }
            CommandPalette => "command_palette",
            NewTab => "new_tab",
            CloseTab => "close_tab",
            ClosePane => "close_pane",
            NextTab => "next_tab",
            PreviousTab => "previous_tab",
            SplitRight => "split_right",
            SplitDown => "split_down",
            TogglePaneZoom => "toggle_pane_zoom",
            RestartShell => "restart_shell",
            ClearScrollback => "clear_scrollback",
            PreviousCommand => "previous_command",
            NextCommand => "next_command",
            CopyLastCommandOutput => "copy_last_output",
            InsertLastOutputIntoAgent => "insert_last_output_into_agent",
            ScrollToTop => "scroll_to_top",
            ScrollToBottom => "scroll_to_bottom",
            ScrollPageUp => "scroll_page_up",
            ScrollPageDown => "scroll_page_down",
            Copy => "copy",
            Paste => "paste",
            FindInTerminal => "find_in_terminal",
            ZoomIn => "zoom_in",
            ZoomOut => "zoom_out",
            ZoomReset => "zoom_reset",
            ToggleExplorer => "toggle_explorer",
            ExplorerToggleHidden => "explorer_toggle_hidden",
            ExplorerToggleFollow => "explorer_toggle_follow",
            ExplorerRefresh => "explorer_refresh",
            ExplorerParent => "explorer_parent",
            ExplorerOpenSelected => "explorer_open_selected",
            ExplorerGoToFolder => "explorer_go_to_folder",
            InsertSelectedPath => "insert_selected_path",
            CopySelectedPath => "copy_selected_path",
            NewTabInSelectedFolder => "new_tab_in_selected_folder",
            OpenSettings => "open_settings",
            ReloadSettings => "reload_settings",
            ToggleAiPanel => "toggle_ai_panel",
            AiRefresh => "ai_refresh",
            OpenPromptStudio => "prompt_studio",
            TogglePromptStudioDock => "prompt_studio_dock",
            OpenPromptsFolder => "open_prompts_folder",
            ToggleGitPanel => "toggle_git_panel",
            GitRefresh => "git_refresh",
            GitShowHistory => "git_history",
            ToggleViewer => "toggle_viewer",
            ViewSelectedFile => "view_selected_file",
            ToggleMonitor => "toggle_monitor",
            KeyboardShortcuts => "keyboard_shortcuts",
            About => "about",
            Website => "website",
            Quit => "quit",
        };
        fixed.to_owned()
    }

    /// Inverse of [`Self::config_name`] (case-insensitive).
    pub fn from_config_name(name: &str) -> Option<CommandId> {
        let name = name.trim().to_ascii_lowercase();
        let parametrised = (0..9)
            .flat_map(|i| [CommandId::NewTabWithShell(i), CommandId::SelectTab(i)])
            .chain(AgentKind::ALL.iter().flat_map(|k| {
                [
                    CommandId::StartAgent(*k, false),
                    CommandId::StartAgent(*k, true),
                ]
            }));
        // The palette itself is not in STATIC_COMMANDS (it does not list itself).
        STATIC_COMMANDS
            .iter()
            .copied()
            .chain(std::iter::once(CommandId::CommandPalette))
            .chain(parametrised)
            .find(|id| id.config_name() == name)
    }

    /// Title for commands that need no runtime data. Parameterised commands get their
    /// title from the app (shell name, tab title).
    pub fn static_title(self) -> &'static str {
        use CommandId::*;
        match self {
            CommandPalette => "Show Command Palette",
            NewTab => "New Tab",
            NewTabWithShell(_) => "New Tab with Shell",
            CloseTab => "Close Tab",
            ClosePane => "Close Pane",
            NextTab => "Next Tab",
            PreviousTab => "Previous Tab",
            SelectTab(_) => "Go to Tab",
            SplitRight => "Split Right",
            SplitDown => "Split Down",
            TogglePaneZoom => "Toggle Pane Zoom",
            FocusPane(Dir::Left) => "Focus Pane Left",
            FocusPane(Dir::Right) => "Focus Pane Right",
            FocusPane(Dir::Up) => "Focus Pane Up",
            FocusPane(Dir::Down) => "Focus Pane Down",
            RestartShell => "Restart Shell",
            ClearScrollback => "Clear Scrollback",
            PreviousCommand => "Go to Previous Command",
            NextCommand => "Go to Next Command",
            CopyLastCommandOutput => "Copy Last Command Output",
            InsertLastOutputIntoAgent => "Insert Last Command Output into Agent",
            ScrollToTop => "Scroll to Top",
            ScrollToBottom => "Scroll to Bottom",
            ScrollPageUp => "Scroll Page Up",
            ScrollPageDown => "Scroll Page Down",
            Copy => "Copy Selection",
            Paste => "Paste",
            FindInTerminal => "Find in Terminal",
            ZoomIn => "Increase Font Size",
            ZoomOut => "Decrease Font Size",
            ZoomReset => "Reset Font Size",
            ToggleExplorer => "Show / Focus / Hide Explorer",
            ExplorerToggleHidden => "Show Hidden Files",
            ExplorerToggleFollow => "Follow Terminal Folder",
            ExplorerRefresh => "Refresh",
            ExplorerParent => "Go to Parent Folder",
            ExplorerOpenSelected => "Open Selected Folder as Root",
            ExplorerGoToFolder => "Go to Folder…",
            InsertSelectedPath => "Insert Selected Path into Terminal",
            CopySelectedPath => "Copy Selected Path",
            NewTabInSelectedFolder => "New Tab in Selected Folder",
            OpenSettings => "Open Settings File",
            ToggleAiPanel => "Toggle AI Workspace",
            AiRefresh => "Refresh Agents and Project Context",
            StartAgent(..) => "Start Agent",
            OpenPromptStudio => "Prompt Studio",
            TogglePromptStudioDock => "Prompt Studio Beside Terminal / Floating",
            OpenPromptsFolder => "Open Prompts Folder",
            ToggleGitPanel => "Toggle Git Panel",
            ToggleViewer => "Toggle Code Viewer",
            ViewSelectedFile => "View Selected File",
            ToggleMonitor => "Toggle Monitor (CPU / Memory)",
            KeyboardShortcuts => "Keyboard Shortcuts",
            GitRefresh => "Refresh Git Status",
            GitShowHistory => "Show History and Activity",
            ReloadSettings => "Reload Settings",
            About => "About ForgeTerm",
            Website => "Open Website (vandeblaak.com)",
            Quit => "Quit ForgeTerm",
        }
    }
}

/// A key, independent of GUI toolkit. Characters are stored lower-case and
/// without Shift applied (Shift is a modifier).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Char(char),
    Tab,
    Enter,
    Insert,
    PageUp,
    PageDown,
    Left,
    Right,
    Up,
    Down,
    /// F1–F12.
    F(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyBinding {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: KeyCode,
}

impl KeyBinding {
    const fn new(ctrl: bool, shift: bool, alt: bool, key: KeyCode) -> Self {
        Self {
            ctrl,
            shift,
            alt,
            key,
        }
    }
}

impl fmt::Display for KeyBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.alt {
            f.write_str("Alt+")?;
        }
        if self.shift {
            f.write_str("Shift+")?;
        }
        match self.key {
            KeyCode::Char(c) => write!(f, "{}", c.to_ascii_uppercase()),
            KeyCode::Tab => f.write_str("Tab"),
            KeyCode::Enter => f.write_str("Enter"),
            KeyCode::Insert => f.write_str("Insert"),
            KeyCode::PageUp => f.write_str("PgUp"),
            KeyCode::PageDown => f.write_str("PgDn"),
            KeyCode::Left => f.write_str("←"),
            KeyCode::Right => f.write_str("→"),
            KeyCode::Up => f.write_str("↑"),
            KeyCode::Down => f.write_str("↓"),
            KeyCode::F(n) => write!(f, "F{n}"),
        }
    }
}

impl KeyBinding {
    /// Parses `"Ctrl+Shift+P"`, `"Alt+Left"`, `"F5"`, `"Ctrl+="` (case-insensitive;
    /// `Control`, `PgUp`/`PageUp` and arrow names are accepted).
    pub fn parse(text: &str) -> Result<KeyBinding, String> {
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        // "Ctrl++" ends in an empty part followed by nothing: the key is "+".
        let (mods, key) = match parts.as_slice() {
            [.., "", ""] => (&parts[..parts.len() - 2], "+"),
            [mods @ .., key] => (mods, *key),
            [] => return Err("empty key".into()),
        };
        let mut b = KeyBinding::new(false, false, false, KeyCode::Tab);
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => b.ctrl = true,
                "shift" => b.shift = true,
                "alt" => b.alt = true,
                other => return Err(format!("unknown modifier '{other}' in '{text}'")),
            }
        }
        let lower = key.to_ascii_lowercase();
        b.key = match lower.as_str() {
            "tab" => KeyCode::Tab,
            "enter" | "return" => KeyCode::Enter,
            "insert" | "ins" => KeyCode::Insert,
            "pageup" | "pgup" => KeyCode::PageUp,
            "pagedown" | "pgdn" => KeyCode::PageDown,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            f if f.len() >= 2
                && f.starts_with('f')
                && f[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)) =>
            {
                KeyCode::F(f[1..].parse().unwrap_or(1))
            }
            _ => {
                let mut chars = lower.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) if !c.is_whitespace() => KeyCode::Char(c),
                    _ => return Err(format!("unknown key '{key}' in '{text}'")),
                }
            }
        };
        Ok(b)
    }

    /// A plain Ctrl+letter (or Ctrl+symbol) is a control code programs use.
    pub fn takes_terminal_key(&self) -> bool {
        self.ctrl && !self.shift && !self.alt && matches!(self.key, KeyCode::Char(_))
    }
}

/// The key map. Order matters only for display: the first binding of a command is
/// the one shown in the palette.
pub struct Keymap {
    bindings: Vec<(KeyBinding, CommandId)>,
}

impl Default for Keymap {
    fn default() -> Self {
        use CommandId::*;
        use KeyCode::*;
        const CS: (bool, bool, bool) = (true, true, false); // Ctrl+Shift
        const C: (bool, bool, bool) = (true, false, false);
        const AS: (bool, bool, bool) = (false, true, true); // Alt+Shift
        const A: (bool, bool, bool) = (false, false, true);
        const S: (bool, bool, bool) = (false, true, false);
        let b = |(c, s, a): (bool, bool, bool), key| KeyBinding::new(c, s, a, key);

        let mut bindings = Vec::new();
        // Windows: Ctrl+V pastes, like Windows Terminal (listed first, so menus show
        // it). Elsewhere Ctrl+V belongs to the program (bash quoted-insert, image
        // paste in Claude Code); Linux terminals paste with Ctrl+Shift+V.
        if cfg!(windows) {
            bindings.push((b(C, Char('v')), Paste));
        }
        bindings.extend([
            (b(CS, Char('p')), CommandPalette),
            (b(CS, Char('t')), NewTab),
            (b(CS, Char('w')), ClosePane),
            (b(CS, Char('e')), ToggleExplorer),
            (b(C, Char(',')), OpenSettings),
            (b(CS, Char('a')), ToggleAiPanel),
            (b(CS, Char('i')), OpenPromptStudio),
            (b(CS, Char('g')), ToggleGitPanel),
            (b(CS, Char('o')), ToggleViewer),
            (b(CS, Char('f')), FindInTerminal),
            (b(CS, Char('m')), ToggleMonitor),
            (b(C, Tab), NextTab),
            (b(CS, Tab), PreviousTab),
            (b(AS, Char('=')), SplitRight),
            (b(AS, Char('+')), SplitRight),
            (b(AS, Char('-')), SplitDown),
            (b(AS, Char('_')), SplitDown),
            (b(CS, Enter), TogglePaneZoom),
            (b(A, Left), FocusPane(Dir::Left)),
            (b(A, Right), FocusPane(Dir::Right)),
            (b(A, Up), FocusPane(Dir::Up)),
            (b(A, Down), FocusPane(Dir::Down)),
            (b(S, PageUp), ScrollPageUp),
            (b(S, PageDown), ScrollPageDown),
            (b(CS, Up), PreviousCommand),
            (b(CS, Down), NextCommand),
            (b(CS, Char('c')), Copy),
            (b(CS, Char('v')), Paste),
            (b(S, Insert), Paste),
            (b(C, Char('=')), ZoomIn),
            (b(C, Char('+')), ZoomIn),
            (b(C, Char('-')), ZoomOut),
            (b(C, Char('0')), ZoomReset),
        ]);
        for n in 1..=9u8 {
            let digit = char::from(b'0' + n);
            let index = usize::from(n - 1);
            bindings.push((b(CS, Char(digit)), NewTabWithShell(index)));
            // Alt+N, not Ctrl+Alt+N: Ctrl+Alt is AltGr on Windows.
            bindings.push((b(A, Char(digit)), SelectTab(index)));
        }
        Self { bindings }
    }
}

impl Keymap {
    /// The defaults plus the user's `[keys]` entries (key text, command name or
    /// `"none"`). A user binding replaces whatever the key did before. Returns the
    /// problems found, as text for the settings banner.
    pub fn with_overrides(overrides: &[(String, String)]) -> (Keymap, Vec<String>) {
        let mut map = Keymap::default();
        let mut problems = Vec::new();
        for (key_text, command) in overrides {
            let key = match KeyBinding::parse(key_text) {
                Ok(k) => k,
                Err(e) => {
                    problems.push(format!("keys: {e}"));
                    continue;
                }
            };
            let command = command.trim();
            map.bindings.retain(|(b, _)| *b != key);
            if command.eq_ignore_ascii_case("none") {
                continue;
            }
            let Some(id) = CommandId::from_config_name(command) else {
                problems.push(format!(
                    "keys: unknown command '{command}' for {key} (see Help → Keyboard Shortcuts)"
                ));
                continue;
            };
            // First, so menus and the palette show the user's choice.
            map.bindings.insert(0, (key, id));
            if key.takes_terminal_key() {
                problems.push(format!(
                    "keys: {key} now runs '{command}' and no longer reaches programs in the terminal"
                ));
            }
        }
        (map, problems)
    }

    /// All bindings of a command, in display order.
    pub fn bindings_for(&self, id: CommandId) -> Vec<KeyBinding> {
        self.bindings
            .iter()
            .filter(|(_, c)| *c == id)
            .map(|(b, _)| *b)
            .collect()
    }

    pub fn lookup(&self, pressed: KeyBinding) -> Option<CommandId> {
        self.bindings
            .iter()
            .find_map(|(binding, id)| (*binding == pressed).then_some(*id))
    }

    /// The primary binding of a command, for display.
    pub fn binding_for(&self, id: CommandId) -> Option<KeyBinding> {
        self.bindings
            .iter()
            .find_map(|(binding, cmd)| (*cmd == id).then_some(*binding))
    }

    /// No two different commands may share a binding.
    #[cfg(test)]
    fn conflicts(&self) -> Vec<(KeyBinding, CommandId, CommandId)> {
        let mut out = Vec::new();
        for (i, (b1, c1)) in self.bindings.iter().enumerate() {
            for (b2, c2) in &self.bindings[i + 1..] {
                if b1 == b2 && c1 != c2 {
                    out.push((*b1, *c1, *c2));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(ctrl: bool, shift: bool, alt: bool, key: KeyCode) -> KeyBinding {
        KeyBinding::new(ctrl, shift, alt, key)
    }

    #[test]
    fn default_keymap_has_no_conflicts() {
        assert!(Keymap::default().conflicts().is_empty());
    }

    #[test]
    fn lookup_finds_commands() {
        let km = Keymap::default();
        assert_eq!(
            km.lookup(key(true, true, false, KeyCode::Char('p'))),
            Some(CommandId::CommandPalette)
        );
        assert_eq!(
            km.lookup(key(false, true, true, KeyCode::Char('='))),
            Some(CommandId::SplitRight)
        );
        assert_eq!(
            km.lookup(key(true, true, false, KeyCode::Char('3'))),
            Some(CommandId::NewTabWithShell(2))
        );
        assert_eq!(
            km.lookup(key(false, false, true, KeyCode::Char('1'))),
            Some(CommandId::SelectTab(0))
        );
    }

    #[test]
    fn terminal_keys_are_not_captured() {
        let km = Keymap::default();
        // Plain typing, Ctrl+C (SIGINT), Ctrl+Alt (AltGr) must reach the terminal.
        for pressed in [
            key(false, false, false, KeyCode::Char('p')),
            key(true, false, false, KeyCode::Char('c')),
            key(true, false, true, KeyCode::Char('1')),
            key(false, false, false, KeyCode::Tab),
            key(false, false, false, KeyCode::Enter),
            key(false, false, false, KeyCode::Up),
        ] {
            assert_eq!(
                km.lookup(pressed),
                None,
                "{pressed} must go to the terminal"
            );
        }
    }

    #[test]
    fn ctrl_v_pastes_on_windows_only() {
        let km = Keymap::default();
        let ctrl_v = km.lookup(key(true, false, false, KeyCode::Char('v')));
        if cfg!(windows) {
            assert_eq!(ctrl_v, Some(CommandId::Paste));
            assert_eq!(
                km.binding_for(CommandId::Paste).unwrap().to_string(),
                "Ctrl+V"
            );
        } else {
            assert_eq!(ctrl_v, None, "Ctrl+V must reach the program on Unix");
        }
        // Available everywhere.
        assert_eq!(
            km.lookup(key(true, true, false, KeyCode::Char('v'))),
            Some(CommandId::Paste)
        );
        assert_eq!(
            km.lookup(key(false, true, false, KeyCode::Insert)),
            Some(CommandId::Paste)
        );
    }

    #[test]
    fn parses_key_text() {
        let p = |s: &str| KeyBinding::parse(s).unwrap();
        assert_eq!(
            p("Ctrl+Shift+P"),
            key(true, true, false, KeyCode::Char('p'))
        );
        assert_eq!(
            p("control + k"),
            key(true, false, false, KeyCode::Char('k'))
        );
        assert_eq!(p("Alt+Left"), key(false, false, true, KeyCode::Left));
        assert_eq!(p("F5"), key(false, false, false, KeyCode::F(5)));
        assert_eq!(p("Shift+F12"), key(false, true, false, KeyCode::F(12)));
        assert_eq!(p("Ctrl+="), key(true, false, false, KeyCode::Char('=')));
        assert_eq!(p("Ctrl++"), key(true, false, false, KeyCode::Char('+')));
        assert_eq!(p("Ctrl+PgDn"), key(true, false, false, KeyCode::PageDown));
        for bad in [
            "",
            "Ctrl+",
            "Hyper+K",
            "Ctrl+Shift+Banana",
            "F13",
            "Ctrl+ab",
        ] {
            assert!(KeyBinding::parse(bad).is_err(), "{bad}");
        }
        // Round trip through the display text.
        assert_eq!(p(&p("Ctrl+Alt+F3").to_string()), p("Ctrl+Alt+F3"));
    }

    #[test]
    fn config_names_are_unique_and_round_trip() {
        let mut all: Vec<CommandId> = STATIC_COMMANDS.to_vec();
        all.push(CommandId::CommandPalette);
        all.extend((0..9).map(CommandId::NewTabWithShell));
        all.extend((0..9).map(CommandId::SelectTab));
        for k in AgentKind::ALL {
            all.push(CommandId::StartAgent(k, false));
            all.push(CommandId::StartAgent(k, true));
        }
        let mut seen = std::collections::HashSet::new();
        for id in all {
            let name = id.config_name();
            assert!(seen.insert(name.clone()), "duplicate {name}");
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            );
            assert_eq!(CommandId::from_config_name(&name), Some(id), "{name}");
        }
        assert_eq!(
            CommandId::from_config_name(" Command_Palette "),
            Some(CommandId::CommandPalette)
        );
        assert_eq!(CommandId::from_config_name("nope"), None);
    }

    #[test]
    fn user_keys_add_replace_and_remove() {
        let o = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(k, c)| ((*k).to_owned(), (*c).to_owned()))
                .collect::<Vec<_>>()
        };
        let (km, problems) = Keymap::with_overrides(&o(&[
            ("Ctrl+K", "command_palette"),
            ("Ctrl+Shift+G", "none"),
            ("F5", "restart_shell"),
            ("Alt+Shift+=", "split_down"),
        ]));
        // Ctrl+K is a terminal control code: allowed, but reported honestly.
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("no longer reaches programs"));
        assert_eq!(
            km.lookup(key(true, false, false, KeyCode::Char('k'))),
            Some(CommandId::CommandPalette)
        );
        // The user's binding is shown first; the default still works.
        assert_eq!(
            km.binding_for(CommandId::CommandPalette)
                .unwrap()
                .to_string(),
            "Ctrl+K"
        );
        assert_eq!(
            km.lookup(key(true, true, false, KeyCode::Char('p'))),
            Some(CommandId::CommandPalette)
        );
        assert_eq!(km.lookup(key(true, true, false, KeyCode::Char('g'))), None);
        assert_eq!(
            km.lookup(key(false, false, false, KeyCode::F(5))),
            Some(CommandId::RestartShell)
        );
        assert_eq!(
            km.lookup(key(false, true, true, KeyCode::Char('='))),
            Some(CommandId::SplitDown)
        );
        assert!(km.conflicts().is_empty());

        let (_, problems) =
            Keymap::with_overrides(&o(&[("Ctrl+Q", "fly_away"), ("Hyper+X", "copy")]));
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].contains("unknown command 'fly_away'"));
        assert!(problems[1].contains("unknown modifier"));
    }

    #[test]
    fn binding_display() {
        let km = Keymap::default();
        assert_eq!(
            km.binding_for(CommandId::CommandPalette)
                .unwrap()
                .to_string(),
            "Ctrl+Shift+P"
        );
        assert_eq!(
            km.binding_for(CommandId::SplitRight).unwrap().to_string(),
            "Alt+Shift+="
        );
        assert_eq!(
            km.binding_for(CommandId::FocusPane(Dir::Left))
                .unwrap()
                .to_string(),
            "Alt+←"
        );
        assert_eq!(km.binding_for(CommandId::RestartShell), None);
    }

    #[test]
    fn every_static_command_has_a_title_and_category() {
        for id in STATIC_COMMANDS {
            assert!(!id.static_title().is_empty());
            assert!(!id.category().is_empty());
        }
    }
}
