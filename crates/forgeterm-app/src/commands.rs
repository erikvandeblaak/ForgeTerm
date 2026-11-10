//! Central command registry: every user-facing action has one `CommandId`, a title, a
//! category and zero or more key bindings. The Command Palette, keyboard handling and
//! the menu bar all read from here, so an action can never exist in one place
//! and be missing or behave differently in another.
//!
//! Pure data and logic — no iced types — so it is unit-tested.

use std::fmt;

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
    ScrollToTop,
    ScrollToBottom,
    ScrollPageUp,
    ScrollPageDown,
    Copy,
    Paste,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    About,
    Quit,
}

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
    CommandId::ScrollToTop,
    CommandId::ScrollToBottom,
    CommandId::ScrollPageUp,
    CommandId::ScrollPageDown,
    CommandId::Copy,
    CommandId::Paste,
    CommandId::ZoomIn,
    CommandId::ZoomOut,
    CommandId::ZoomReset,
    CommandId::About,
    CommandId::Quit,
];

impl CommandId {
    pub fn category(self) -> &'static str {
        use CommandId::*;
        match self {
            CommandPalette | ZoomIn | ZoomOut | ZoomReset => "View",
            NewTab | NewTabWithShell(_) | CloseTab | NextTab | PreviousTab | SelectTab(_) => "Tab",
            SplitRight | SplitDown | ClosePane | TogglePaneZoom | FocusPane(_) => "Pane",
            RestartShell | ClearScrollback | ScrollToTop | ScrollToBottom | ScrollPageUp
            | ScrollPageDown => "Terminal",
            Copy | Paste => "Edit",
            About => "Help",
            Quit => "App",
        }
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
            ScrollToTop => "Scroll to Top",
            ScrollToBottom => "Scroll to Bottom",
            ScrollPageUp => "Scroll Page Up",
            ScrollPageDown => "Scroll Page Down",
            Copy => "Copy Selection",
            Paste => "Paste",
            ZoomIn => "Increase Font Size",
            ZoomOut => "Decrease Font Size",
            ZoomReset => "Reset Font Size",
            About => "About ForgeTerm",
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
        }
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

        let mut bindings = vec![
            (b(CS, Char('p')), CommandPalette),
            (b(CS, Char('t')), NewTab),
            (b(CS, Char('w')), ClosePane),
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
            (b(CS, Char('c')), Copy),
            (b(CS, Char('v')), Paste),
            (b(S, Insert), Paste),
            (b(C, Char('=')), ZoomIn),
            (b(C, Char('+')), ZoomIn),
            (b(C, Char('-')), ZoomOut),
            (b(C, Char('0')), ZoomReset),
        ];
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
            key(true, false, false, KeyCode::Char('v')),
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
