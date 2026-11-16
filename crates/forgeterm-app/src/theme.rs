//! Forge design tokens (see docs/DESIGN_SYSTEM.md): the single source for UI, terminal
//! and code colors, plus type sizes, spacing and radii.
//!
//! Colors belong to the active theme ([`ThemeChoice`]), so they are functions:
//! `theme::text()` reads the current theme. The app sets the theme once per change
//! ([`set`]) and redraws; nothing else writes it. Sizes are the same in every theme.

use std::sync::atomic::{AtomicUsize, Ordering};

use forgeterm_settings::ThemeChoice;
use forgeterm_terminal::{Palette as TermPalette, Rgb};
use forgeterm_viewer::CodeRole;
use iced::Color;
use iced::theme::Palette;

const fn hex(v: u32) -> Color {
    Color::from_rgb8((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

const fn ansi(v: [u32; 16]) -> [Rgb; 16] {
    let mut out = [Rgb::hex(0); 16];
    let mut i = 0;
    while i < 16 {
        out[i] = Rgb::hex(v[i]);
        i += 1;
    }
    out
}

/// One theme's colors.
pub struct Tokens {
    pub choice: ThemeChoice,
    /// Terminal and app background.
    pub obsidian: Color,
    /// Top bar, status bar.
    pub graphite: Color,
    /// Banners, panels.
    pub surface: Color,
    /// Buttons, popovers.
    pub elevated: Color,
    pub border: Color,
    pub selection: Color,
    pub text: Color,
    pub text_muted: Color,
    /// Disabled menu entries (WCAG exempts disabled controls; still legible).
    pub disabled: Color,
    /// Primary, focus, cursor.
    pub cyan: Color,
    /// Success, running.
    pub mint: Color,
    /// Informational.
    pub violet: Color,
    /// Warning.
    pub amber: Color,
    /// Error.
    pub coral: Color,
    /// Chart bars: the primary step that keeps >= 3:1 against the bars' backgrounds.
    pub chart_cyan: Color,
    /// Code colors by [`CodeRole`] (in [`CodeRole::ALL`] order).
    pub code: [Color; 10],
    pub terminal: TermPalette,
}

/// Midnight: the original Forge Dark.
const MIDNIGHT: Tokens = Tokens {
    choice: ThemeChoice::Midnight,
    obsidian: hex(0x090D14),
    graphite: hex(0x111827),
    surface: hex(0x172132),
    elevated: hex(0x202C40),
    border: hex(0x253247),
    selection: hex(0x2B4466),
    text: hex(0xD6DEEB),
    text_muted: hex(0x8B98AD),
    disabled: hex(0x5F6B80),
    cyan: hex(0x4DD8E8),
    mint: hex(0x55E6B5),
    violet: hex(0xA78BFA),
    amber: hex(0xF5C978),
    coral: hex(0xF08080),
    chart_cyan: hex(0x26A6B6),
    code: [
        hex(0xD6DEEB), // text
        hex(0x6B7890), // comment
        hex(0x55E6B5), // string
        hex(0xF5C978), // constant
        hex(0xA78BFA), // keyword
        hex(0xC9D3E3), // operator
        hex(0x4DD8E8), // function
        hex(0x7CC4FA), // type
        hex(0xF08080), // error
        hex(0xE6B8A2), // parameter
    ],
    terminal: TermPalette {
        ansi: ansi([
            0x202C40, 0xF08080, 0x55E6B5, 0xF5C978, 0x3B6FD8, 0xA78BFA, 0x4DD8E8, 0xC8D3E3,
            0x5B6B85, 0xFF9E9E, 0x7DF0C8, 0xFFDDA0, 0x93C1FF, 0xC4B2FF, 0x82E8F2, 0xF2F6FB,
        ]),
        foreground: Rgb::hex(0xD6DEEB),
        background: Rgb::hex(0x090D14),
        cursor: Rgb::hex(0x4DD8E8),
        selection_bg: Rgb::hex(0x2B4466),
        selection_fg: Rgb::hex(0xF2F6FB),
    },
};

/// Graphite: neutral grey without the blue cast, calmer accents.
const GRAPHITE: Tokens = Tokens {
    choice: ThemeChoice::Graphite,
    obsidian: hex(0x141619),
    graphite: hex(0x1B1E22),
    surface: hex(0x22262B),
    elevated: hex(0x2C3137),
    border: hex(0x353B42),
    selection: hex(0x3A4553),
    text: hex(0xDCDFE4),
    text_muted: hex(0x9BA1AB),
    disabled: hex(0x6A717B),
    cyan: hex(0x5EC4D1),
    mint: hex(0x6FD1A6),
    violet: hex(0xAE9CEB),
    amber: hex(0xE2BE7E),
    coral: hex(0xE88E89),
    chart_cyan: hex(0x3199A6),
    code: [
        hex(0xDCDFE4),
        hex(0x7A818C),
        hex(0x6FD1A6),
        hex(0xE2BE7E),
        hex(0xAE9CEB),
        hex(0xC3C8CF),
        hex(0x5EC4D1),
        hex(0x86B5E8),
        hex(0xE88E89),
        hex(0xDDB6A0),
    ],
    terminal: TermPalette {
        ansi: ansi([
            0x2C3137, 0xE88E89, 0x6FD1A6, 0xE2BE7E, 0x4A72C4, 0xAE9CEB, 0x5EC4D1, 0xC9CDD4,
            0x60676F, 0xF4A9A5, 0x91E2BF, 0xEFD2A0, 0x97B8F2, 0xC8BCF5, 0x8DD8E1, 0xF2F3F5,
        ]),
        foreground: Rgb::hex(0xDCDFE4),
        background: Rgb::hex(0x141619),
        cursor: Rgb::hex(0x5EC4D1),
        selection_bg: Rgb::hex(0x3A4553),
        selection_fg: Rgb::hex(0xF2F3F5),
    },
};

/// Arctic: light. Accents are darker so they read on white; filled accent buttons
/// carry the (light) background color as text.
const ARCTIC: Tokens = Tokens {
    choice: ThemeChoice::Arctic,
    obsidian: hex(0xF7F9FC),
    graphite: hex(0xECF0F5),
    surface: hex(0xFFFFFF),
    elevated: hex(0xE2E8F0),
    border: hex(0xC9D2DE),
    selection: hex(0xC7DCF4),
    text: hex(0x1B2431),
    text_muted: hex(0x4E5A6B),
    disabled: hex(0x8A94A3),
    cyan: hex(0x007A8A),
    mint: hex(0x0A7A53),
    violet: hex(0x6548C4),
    amber: hex(0x8A5800),
    coral: hex(0xBF3732),
    chart_cyan: hex(0x1C8E9C),
    code: [
        hex(0x1B2431),
        hex(0x687487),
        hex(0x0A7A53),
        hex(0x8A5800),
        hex(0x6548C4),
        hex(0x3A4557),
        hex(0x006D7C),
        hex(0x2456C2),
        hex(0xBF3732),
        hex(0x974C2B),
    ],
    terminal: TermPalette {
        ansi: ansi([
            0x1B2431, 0xBF3732, 0x0A7A53, 0x8A5800, 0x2456C2, 0x7A3FB0, 0x007A8A, 0xC9D1DC,
            0x5B6779, 0xD14C47, 0x138A60, 0x9E6600, 0x3A6BD9, 0x8E52C8, 0x0A8C9E, 0xE6EBF1,
        ]),
        foreground: Rgb::hex(0x1B2431),
        background: Rgb::hex(0xF7F9FC),
        cursor: Rgb::hex(0x007A8A),
        selection_bg: Rgb::hex(0xC7DCF4),
        selection_fg: Rgb::hex(0x1B2431),
    },
};

/// Cyber: violet-black with neon accents.
const CYBER: Tokens = Tokens {
    choice: ThemeChoice::Cyber,
    obsidian: hex(0x0A0714),
    graphite: hex(0x110C20),
    surface: hex(0x18122C),
    elevated: hex(0x231B3E),
    border: hex(0x2F2552),
    selection: hex(0x3D2C73),
    text: hex(0xECE6FF),
    text_muted: hex(0xA99ECC),
    disabled: hex(0x6F6596),
    cyan: hex(0x00E0F5),
    mint: hex(0x2EF2A0),
    violet: hex(0xC77DFF),
    amber: hex(0xFFD15C),
    coral: hex(0xFF5D8F),
    chart_cyan: hex(0x00A8BB),
    code: [
        hex(0xECE6FF),
        hex(0x7F74A8),
        hex(0x2EF2A0),
        hex(0xFFD15C),
        hex(0xFF6AD5),
        hex(0xCFC6EC),
        hex(0x00E0F5),
        hex(0x9C9CFF),
        hex(0xFF5D8F),
        hex(0xFFB48A),
    ],
    terminal: TermPalette {
        ansi: ansi([
            0x231B3E, 0xFF5D8F, 0x2EF2A0, 0xFFD15C, 0x5552E0, 0xC77DFF, 0x00E0F5, 0xD3CBEE,
            0x5F5386, 0xFF8AB0, 0x7DF8C4, 0xFFE08F, 0x9C9CFF, 0xDCA8FF, 0x6CF0FA, 0xF7F4FF,
        ]),
        foreground: Rgb::hex(0xECE6FF),
        background: Rgb::hex(0x0A0714),
        cursor: Rgb::hex(0x00E0F5),
        selection_bg: Rgb::hex(0x3D2C73),
        selection_fg: Rgb::hex(0xF7F4FF),
    },
};

/// High Contrast: black, white, strong accents and borders that stand out.
const HIGH_CONTRAST: Tokens = Tokens {
    choice: ThemeChoice::HighContrast,
    obsidian: hex(0x000000),
    graphite: hex(0x0A0A0A),
    surface: hex(0x111111),
    elevated: hex(0x1F1F1F),
    border: hex(0x9A9A9A),
    selection: hex(0x0F4C9E),
    text: hex(0xFFFFFF),
    text_muted: hex(0xD4D4D4),
    disabled: hex(0x9A9A9A),
    cyan: hex(0x33E6FF),
    mint: hex(0x4DFF9A),
    violet: hex(0xD0B0FF),
    amber: hex(0xFFD84D),
    coral: hex(0xFF7A7A),
    chart_cyan: hex(0x00CDE6),
    code: [
        hex(0xFFFFFF),
        hex(0xB4B4B4),
        hex(0x4DFF9A),
        hex(0xFFD84D),
        hex(0xD0B0FF),
        hex(0xE6E6E6),
        hex(0x33E6FF),
        hex(0x8CB4FF),
        hex(0xFF7A7A),
        hex(0xFFB48A),
    ],
    terminal: TermPalette {
        ansi: ansi([
            0x000000, 0xFF6B6B, 0x4DFF9A, 0xFFE34D, 0x2F5FD0, 0xE08CFF, 0x33E6FF, 0xE6E6E6,
            0x8C8C8C, 0xFF9A9A, 0x8CFFBE, 0xFFF08C, 0x8CB4FF, 0xF0B8FF, 0x8CF2FF, 0xFFFFFF,
        ]),
        foreground: Rgb::hex(0xFFFFFF),
        background: Rgb::hex(0x000000),
        cursor: Rgb::hex(0xFFD84D),
        selection_bg: Rgb::hex(0x0F4C9E),
        selection_fg: Rgb::hex(0xFFFFFF),
    },
};

static THEMES: [Tokens; 5] = [MIDNIGHT, GRAPHITE, ARCTIC, CYBER, HIGH_CONTRAST];

static CURRENT: AtomicUsize = AtomicUsize::new(0);

fn index(choice: ThemeChoice) -> usize {
    THEMES
        .iter()
        .position(|t| t.choice == choice)
        .expect("every theme choice has tokens")
}

/// Makes `choice` the active theme. The caller redraws (and refreshes terminals).
pub fn set(choice: ThemeChoice) {
    CURRENT.store(index(choice), Ordering::Relaxed);
}

pub fn current() -> ThemeChoice {
    tokens().choice
}

/// The active theme's tokens.
pub fn tokens() -> &'static Tokens {
    &THEMES[CURRENT.load(Ordering::Relaxed)]
}

macro_rules! color_fns {
    ($($name:ident),* $(,)?) => {
        $(
            #[inline]
            pub fn $name() -> Color {
                tokens().$name
            }
        )*
    };
}

color_fns!(
    obsidian, graphite, surface, elevated, border, selection, text, text_muted, disabled, cyan,
    mint, violet, amber, coral, chart_cyan,
);

/// Code color for a highlighted span.
pub fn code(role: CodeRole) -> Color {
    let i = CodeRole::ALL.iter().position(|r| *r == role).unwrap_or(0);
    tokens().code[i]
}

/// Terminal colors of the active theme.
pub fn terminal_palette() -> TermPalette {
    tokens().terminal.clone()
}

/// The iced theme (widget defaults) of the active theme.
pub fn iced_theme() -> iced::Theme {
    let t = tokens();
    iced::Theme::custom(
        t.choice.name(),
        Palette {
            background: t.obsidian,
            text: t.text,
            primary: t.cyan,
            success: t.mint,
            warning: t.amber,
            danger: t.coral,
        },
    )
}

pub fn to_color(c: Rgb) -> Color {
    Color::from_rgb8(c.r, c.g, c.b)
}

/// Type sizes (logical pixels). The terminal font size is a setting; one-off
/// display sizes (big numbers, the About logo) stay at their use.
pub mod size {
    /// Badges, axis labels.
    pub const CAPTION: f32 = 10.0;
    /// Counters, hints, secondary lines in lists.
    pub const TINY: f32 = 11.0;
    /// Labels, list rows, buttons.
    pub const SMALL: f32 = 12.0;
    /// Primary UI text.
    pub const BODY: f32 = 13.0;
    /// Emphasised body text.
    pub const LARGE: f32 = 14.0;
    /// Dialog and overlay titles.
    pub const TITLE: f32 = 15.0;
    /// Section headings in dashboards.
    pub const HEADING: f32 = 18.0;
}

/// Spacing scale between items (logical pixels).
pub mod space {
    pub const XXS: f32 = 2.0;
    pub const XS: f32 = 4.0;
    pub const S: f32 = 6.0;
    pub const M: f32 = 8.0;
    pub const L: f32 = 10.0;
    pub const XL: f32 = 12.0;
    pub const XXL: f32 = 16.0;
}

/// Corner radii (logical pixels). Round dots use half their size.
pub mod radius {
    /// Badges, chips, small bars.
    pub const S: f32 = 4.0;
    /// Buttons, inputs, menu rows.
    pub const M: f32 = 6.0;
    /// Popovers, cards.
    pub const L: f32 = 8.0;
    /// Panels and dialogs.
    pub const XL: f32 = 10.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Color) -> f32 {
        let f = |v: f32| {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
    }

    fn contrast(a: Color, b: Color) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn midnight_is_the_original_forge_dark() {
        assert_eq!(MIDNIGHT.terminal, TermPalette::forge_dark());
        assert_eq!(THEMES[0].choice, ThemeChoice::Midnight);
        assert_eq!(index(ThemeChoice::default()), 0);
    }

    #[test]
    fn every_choice_has_tokens_in_order() {
        for (i, choice) in ThemeChoice::ALL.into_iter().enumerate() {
            assert_eq!(THEMES[i].choice, choice);
        }
    }

    /// WCAG AA for text (4.5:1) on every surface it appears on; 3:1 for accents used
    /// as text and for chart bars.
    #[test]
    fn contrast_meets_the_design_system() {
        for t in &THEMES {
            let name = t.choice.name();
            for (bg_name, bg) in [
                ("obsidian", t.obsidian),
                ("graphite", t.graphite),
                ("surface", t.surface),
                ("elevated", t.elevated),
            ] {
                let c = contrast(t.text, bg);
                assert!(c >= 7.0, "{name}: text on {bg_name} {c:.2}");
                let c = contrast(t.text_muted, bg);
                assert!(c >= 4.5, "{name}: muted on {bg_name} {c:.2}");
                for (accent, color) in [
                    ("cyan", t.cyan),
                    ("mint", t.mint),
                    ("violet", t.violet),
                    ("amber", t.amber),
                    ("coral", t.coral),
                ] {
                    let c = contrast(color, bg);
                    assert!(c >= 3.0, "{name}: {accent} on {bg_name} {c:.2}");
                }
            }
            let c = contrast(t.text, t.selection);
            assert!(c >= 4.5, "{name}: text on selection {c:.2}");
            let c = contrast(t.disabled, t.surface);
            assert!(c >= 2.5, "{name}: disabled on surface {c:.2}");
            for bg in [t.graphite, t.obsidian] {
                let c = contrast(t.chart_cyan, bg);
                assert!(c >= 3.0, "{name}: chart bars {c:.2}");
            }
            // Filled accent buttons carry the background color as text.
            let c = contrast(t.obsidian, t.cyan);
            assert!(c >= 4.5, "{name}: text on a cyan button {c:.2}");
            // Terminal: foreground and the normal colors on the background.
            let p = &t.terminal;
            let bg = to_color(p.background);
            let c = contrast(to_color(p.foreground), bg);
            assert!(c >= 7.0, "{name}: terminal text {c:.2}");
            for i in [1, 2, 3, 5, 6] {
                let c = contrast(to_color(p.ansi[i]), bg);
                assert!(c >= 3.0, "{name}: ansi {i} {c:.2}");
            }
            let c = contrast(to_color(p.selection_fg), to_color(p.selection_bg));
            assert!(c >= 4.5, "{name}: terminal selection {c:.2}");
            for (i, color) in t.code.iter().enumerate() {
                let c = contrast(*color, t.obsidian);
                assert!(c >= 3.0, "{name}: code role {i} {c:.2}");
            }
        }
    }
}
