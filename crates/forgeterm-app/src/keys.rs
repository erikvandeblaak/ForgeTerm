//! Maps iced keyboard events to terminal keys.

use forgeterm_terminal::{Key, Modifiers};
use iced::keyboard::{self, key::Named};

pub fn modifiers(m: keyboard::Modifiers) -> Modifiers {
    Modifiers {
        shift: m.shift(),
        ctrl: m.control(),
        alt: m.alt(),
    }
}

/// Translates a key press into a terminal key plus the modifiers to encode with.
pub fn translate(
    key: &keyboard::Key,
    text: Option<&str>,
    mods: keyboard::Modifiers,
) -> Option<(Key, Modifiers)> {
    let m = modifiers(mods);
    let k = match key {
        keyboard::Key::Named(named) => match named {
            Named::Enter => Key::Enter,
            Named::Backspace => Key::Backspace,
            Named::Tab => Key::Tab,
            Named::Escape => Key::Escape,
            Named::Space => Key::Text(" ".into()),
            Named::ArrowUp => Key::Up,
            Named::ArrowDown => Key::Down,
            Named::ArrowLeft => Key::Left,
            Named::ArrowRight => Key::Right,
            Named::Home => Key::Home,
            Named::End => Key::End,
            Named::PageUp => Key::PageUp,
            Named::PageDown => Key::PageDown,
            Named::Insert => Key::Insert,
            Named::Delete => Key::Delete,
            Named::F1 => Key::F(1),
            Named::F2 => Key::F(2),
            Named::F3 => Key::F(3),
            Named::F4 => Key::F(4),
            Named::F5 => Key::F(5),
            Named::F6 => Key::F(6),
            Named::F7 => Key::F(7),
            Named::F8 => Key::F(8),
            Named::F9 => Key::F(9),
            Named::F10 => Key::F(10),
            Named::F11 => Key::F(11),
            Named::F12 => Key::F(12),
            _ => return None,
        },
        keyboard::Key::Character(c) => {
            // AltGr arrives as Ctrl+Alt on Windows: trust the produced text then.
            let produced = text.filter(|t| !t.is_empty() && !t.chars().any(char::is_control));
            if m.ctrl && m.alt && produced.is_some() {
                return Some((Key::Text(produced?.to_owned()), Modifiers::default()));
            }
            if m.ctrl || m.alt {
                Key::Text(c.to_string())
            } else {
                // Shift is already applied to `text`; don't encode it again.
                return Some((
                    Key::Text(produced.unwrap_or(c.as_str()).to_owned()),
                    Modifiers::default(),
                ));
            }
        }
        keyboard::Key::Unidentified => return None,
    };
    Some((k, m))
}
