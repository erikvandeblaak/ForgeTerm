//! The window / taskbar icon, rendered from the ForgeTerm logo (SVG) at startup.

use resvg::{tiny_skia, usvg};

const LOGO: &[u8] = include_bytes!("../../../assets/branding/forgeterm-logo.svg");
/// Pixels per side; Windows scales it down for the title bar and taskbar.
pub const SIZE: u32 = 256;

/// Straight (not premultiplied) RGBA pixels of the logo, or `None` if it cannot be
/// rendered (then the platform's default icon is used).
pub fn logo_rgba(size: u32) -> Option<Vec<u8>> {
    let tree = usvg::Tree::from_data(LOGO, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
    let scale = size as f32 / tree.size().width().max(tree.size().height());
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some(
        pixmap
            .pixels()
            .iter()
            .flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect(),
    )
}

pub fn window_icon() -> Option<iced::window::Icon> {
    iced::window::icon::from_rgba(logo_rgba(SIZE)?, SIZE, SIZE).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_renders_with_its_colors() {
        let px = logo_rgba(64).expect("logo renders");
        assert_eq!(px.len(), 64 * 64 * 4);
        let at = |x: usize, y: usize| &px[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
        // Corner outside the rounded square: transparent.
        assert_eq!(at(0, 0)[3], 0);
        // Inside the square, away from the mark: the dark background #111827.
        assert_eq!(at(8, 32), [0x11, 0x18, 0x27, 0xFF]);
        // The cursor bar of the "F" (#55E6B5).
        assert_eq!(at(39, 46), [0x55, 0xE6, 0xB5, 0xFF]);
        assert!(window_icon().is_some());
    }
}
