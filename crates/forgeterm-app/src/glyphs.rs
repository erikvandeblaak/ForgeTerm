//! Pixel-exact drawing of box-drawing (U+2500..) and block element (U+2580..) characters.
//!
//! Fonts draw these inside the em box, which leaves gaps when the line height is larger
//! than the font's. Terminals therefore draw them as geometry so borders and bars connect.

use iced::widget::canvas::Frame;
use iced::{Color, Point, Size};

/// Line weight per direction.
#[derive(Clone, Copy, PartialEq, Eq)]
enum W {
    None,
    Light,
    Heavy,
    Double,
}

/// (up, down, left, right) for supported line characters.
fn line_spec(c: char) -> Option<[W; 4]> {
    use W::{Double as D, Heavy as H, Light as L, None as N};
    Some(match c {
        '─' => [N, N, L, L],
        '━' => [N, N, H, H],
        '│' => [L, L, N, N],
        '┃' => [H, H, N, N],
        '┌' | '╭' => [N, L, N, L],
        '┐' | '╮' => [N, L, L, N],
        '└' | '╰' => [L, N, N, L],
        '┘' | '╯' => [L, N, L, N],
        '┏' => [N, H, N, H],
        '┓' => [N, H, H, N],
        '┗' => [H, N, N, H],
        '┛' => [H, N, H, N],
        '├' => [L, L, N, L],
        '┤' => [L, L, L, N],
        '┬' => [N, L, L, L],
        '┴' => [L, N, L, L],
        '┼' => [L, L, L, L],
        '┣' => [H, H, N, H],
        '┫' => [H, H, H, N],
        '┳' => [N, H, H, H],
        '┻' => [H, N, H, H],
        '╋' => [H, H, H, H],
        '═' => [N, N, D, D],
        '║' => [D, D, N, N],
        '╔' => [N, D, N, D],
        '╗' => [N, D, D, N],
        '╚' => [D, N, N, D],
        '╝' => [D, N, D, N],
        '╠' => [D, D, N, D],
        '╣' => [D, D, D, N],
        '╦' => [N, D, D, D],
        '╩' => [D, N, D, D],
        '╬' => [D, D, D, D],
        '╴' => [N, N, L, N],
        '╵' => [L, N, N, N],
        '╶' => [N, N, N, L],
        '╷' => [N, L, N, N],
        _ => return None,
    })
}

/// Draws `c` into the cell at `origin` if it is a supported box/block character.
/// Returns `false` when the caller should render it as text instead.
pub fn draw(frame: &mut Frame, c: char, origin: Point, cell: Size, color: Color) -> bool {
    let (x, y, w, h) = (origin.x, origin.y, cell.width, cell.height);
    let mut rect = |x0: f32, y0: f32, x1: f32, y1: f32| {
        frame.fill_rectangle(
            Point::new(x0, y0),
            Size::new((x1 - x0).max(0.0), (y1 - y0).max(0.0)),
            color,
        );
    };

    if let Some([up, down, left, right]) = line_spec(c) {
        let light = (w / 8.0).round().max(1.0);
        let heavy = (light * 2.0).max(2.0);
        let cx = (x + w / 2.0 - light / 2.0).round();
        let cy = (y + h / 2.0 - light / 2.0).round();
        let gap = light.max(1.0) + 1.0;

        let thick = |wt: W| if wt == W::Heavy { heavy } else { light };
        // Vertical strokes.
        for (wt, y0, y1) in [(up, y, cy + light), (down, cy, y + h)] {
            match wt {
                W::None => {}
                W::Double => {
                    rect(cx - gap, y0, cx - gap + light, y1);
                    rect(cx + gap, y0, cx + gap + light, y1);
                }
                _ => {
                    let t = thick(wt);
                    let x0 = cx + light / 2.0 - t / 2.0;
                    rect(x0, y0, x0 + t, y1);
                }
            }
        }
        // Horizontal strokes.
        for (wt, x0, x1) in [(left, x, cx + light), (right, cx, x + w)] {
            match wt {
                W::None => {}
                W::Double => {
                    rect(x0, cy - gap, x1, cy - gap + light);
                    rect(x0, cy + gap, x1, cy + gap + light);
                }
                _ => {
                    let t = thick(wt);
                    let y0 = cy + light / 2.0 - t / 2.0;
                    rect(x0, y0, x1, y0 + t);
                }
            }
        }
        return true;
    }

    let eighth_h = h / 8.0;
    let eighth_w = w / 8.0;
    match c {
        '█' => rect(x, y, x + w, y + h),
        '▀' => rect(x, y, x + w, y + h / 2.0),
        '▄' => rect(x, y + h / 2.0, x + w, y + h),
        '▌' => rect(x, y, x + w / 2.0, y + h),
        '▐' => rect(x + w / 2.0, y, x + w, y + h),
        '▔' => rect(x, y, x + w, y + eighth_h),
        '▕' => rect(x + w - eighth_w, y, x + w, y + h),
        // Lower n/8 blocks: ▁▂▃▄▅▆▇ (▄ handled above).
        '▁'..='▇' => {
            let n = (c as u32 - '▁' as u32 + 1) as f32;
            rect(x, y + h - n * eighth_h, x + w, y + h);
        }
        // Left n/8 blocks: ▉▊▋▌▍▎▏ (▌ handled above).
        '▉'..='▏' => {
            let n = (8 - (c as u32 - '▉' as u32 + 1)) as f32;
            rect(x, y, x + n * eighth_w, y + h);
        }
        '░' | '▒' | '▓' => {
            let alpha = match c {
                '░' => 0.25,
                '▒' => 0.5,
                _ => 0.75,
            };
            frame.fill_rectangle(
                origin,
                cell,
                Color {
                    a: color.a * alpha,
                    ..color
                },
            );
        }
        _ => return false,
    }
    true
}

/// Characters that should never be merged into text runs.
pub fn is_box_or_block(c: char) -> bool {
    ('\u{2500}'..='\u{259F}').contains(&c)
}
