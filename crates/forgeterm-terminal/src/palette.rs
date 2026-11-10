use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::Color;

/// 24-bit color, independent of any GUI toolkit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn hex(v: u32) -> Self {
        Self {
            r: (v >> 16) as u8,
            g: (v >> 8) as u8,
            b: v as u8,
        }
    }

    /// Scales brightness, used for SGR 2 (dim).
    pub fn dim(self) -> Self {
        let f = |c: u8| (f32::from(c) * 0.66) as u8;
        Self {
            r: f(self.r),
            g: f(self.g),
            b: f(self.b),
        }
    }
}

/// Terminal color scheme: 16 ANSI colors plus special colors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub ansi: [Rgb; 16],
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub selection_bg: Rgb,
    pub selection_fg: Rgb,
}

impl Palette {
    /// "Forge Dark" — the default ForgeTerm scheme (see docs/DESIGN_SYSTEM.md).
    pub fn forge_dark() -> Self {
        Self {
            ansi: [
                Rgb::hex(0x202C40), // black
                Rgb::hex(0xF08080), // red      (Coral Red)
                Rgb::hex(0x55E6B5), // green    (Emerald Mint)
                Rgb::hex(0xF5C978), // yellow   (Warm Amber)
                Rgb::hex(0x3B6FD8), // blue (dark enough to work as a background)
                Rgb::hex(0xA78BFA), // magenta  (Royal Violet)
                Rgb::hex(0x4DD8E8), // cyan     (Electric Cyan)
                Rgb::hex(0xC8D3E3), // white
                Rgb::hex(0x5B6B85), // bright black
                Rgb::hex(0xFF9E9E),
                Rgb::hex(0x7DF0C8),
                Rgb::hex(0xFFDDA0),
                Rgb::hex(0x93C1FF),
                Rgb::hex(0xC4B2FF),
                Rgb::hex(0x82E8F2),
                Rgb::hex(0xF2F6FB), // bright white
            ],
            foreground: Rgb::hex(0xD6DEEB),
            background: Rgb::hex(0x090D14),
            cursor: Rgb::hex(0x4DD8E8),
            selection_bg: Rgb::hex(0x2B4466),
            selection_fg: Rgb::hex(0xF2F6FB),
        }
    }

    /// Color for an xterm 256-color index or alacritty "named" index (>= 256).
    pub fn by_index(&self, index: usize) -> Rgb {
        match index {
            0..=15 => self.ansi[index],
            16..=231 => {
                let i = index - 16;
                let level = |v: usize| if v == 0 { 0 } else { (55 + 40 * v) as u8 };
                Rgb {
                    r: level(i / 36),
                    g: level((i / 6) % 6),
                    b: level(i % 6),
                }
            }
            232..=255 => {
                let v = (8 + 10 * (index - 232)) as u8;
                Rgb { r: v, g: v, b: v }
            }
            256 => self.foreground,
            257 => self.background,
            258 => self.cursor,
            259..=266 => self.ansi[index - 259].dim(),
            267 => self.ansi[15],
            268 => self.foreground.dim(),
            _ => self.foreground,
        }
    }

    /// Resolves a terminal color, honouring runtime overrides set via OSC 4/10/11.
    pub(crate) fn resolve(&self, color: Color, overrides: &Colors) -> Rgb {
        let index = match color {
            Color::Spec(rgb) => {
                return Rgb {
                    r: rgb.r,
                    g: rgb.g,
                    b: rgb.b,
                };
            }
            Color::Named(named) => named as usize,
            Color::Indexed(i) => usize::from(i),
        };
        match overrides[index] {
            Some(rgb) => Rgb {
                r: rgb.r,
                g: rgb.g,
                b: rgb.b,
            },
            None => self.by_index(index),
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::forge_dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_cube_and_grayscale() {
        let p = Palette::forge_dark();
        assert_eq!(p.by_index(16), Rgb::hex(0x000000));
        assert_eq!(p.by_index(231), Rgb::hex(0xFFFFFF));
        assert_eq!(p.by_index(196), Rgb::hex(0xFF0000));
        assert_eq!(p.by_index(232), Rgb::hex(0x080808));
        assert_eq!(p.by_index(255), Rgb::hex(0xEEEEEE));
    }

    #[test]
    fn named_specials() {
        let p = Palette::forge_dark();
        assert_eq!(p.by_index(256), p.foreground);
        assert_eq!(p.by_index(257), p.background);
        assert_eq!(p.by_index(1), Rgb::hex(0xF08080));
    }
}
