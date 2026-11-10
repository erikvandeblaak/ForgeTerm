//! Forge design tokens (see docs/DESIGN_SYSTEM.md). Single source for UI colors.
//! The full token set is defined up front; not every token has a consumer yet.
#![allow(dead_code)]

use iced::Color;
use iced::theme::Palette;

const fn hex(v: u32) -> Color {
    Color::from_rgb8((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pub const OBSIDIAN: Color = hex(0x090D14);
pub const GRAPHITE: Color = hex(0x111827);
pub const SURFACE: Color = hex(0x172132);
pub const ELEVATED: Color = hex(0x202C40);

pub const CYAN: Color = hex(0x4DD8E8);
pub const MINT: Color = hex(0x55E6B5);
pub const VIOLET: Color = hex(0xA78BFA);
pub const AMBER: Color = hex(0xF5C978);
pub const CORAL: Color = hex(0xF08080);

pub const TEXT: Color = hex(0xD6DEEB);
pub const TEXT_MUTED: Color = hex(0x8B98AD);
pub const BORDER: Color = hex(0x253247);

pub fn forge_dark() -> iced::Theme {
    iced::Theme::custom(
        "Forge Dark",
        Palette {
            background: OBSIDIAN,
            text: TEXT,
            primary: CYAN,
            success: MINT,
            warning: AMBER,
            danger: CORAL,
        },
    )
}

pub fn to_color(c: forgeterm_terminal::Rgb) -> Color {
    Color::from_rgb8(c.r, c.g, c.b)
}
