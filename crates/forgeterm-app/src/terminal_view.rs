//! Canvas widget that renders a terminal [`Snapshot`] and turns mouse input into
//! messages (selection, scrolling, mouse reporting).

use forgeterm_terminal::{CellView, CursorShape, Modes, MouseButton, Snapshot, encode_mouse};
use iced::alignment::Vertical;
use iced::font::{Style, Weight};
use iced::mouse::{self, Cursor, ScrollDelta};
use iced::widget::canvas::{self, Action, Cache, Event, Frame, Geometry, Text};
use iced::widget::text::LineHeight;
use iced::{Font, Pixels, Point, Rectangle, Renderer, Size, Theme, window};

use crate::app::Message;
use crate::glyphs;
use crate::terminal_pane::{TermId, TermMsg};
use crate::theme::to_color;

pub const TERMINAL_FONT: Font = Font::with_name("JetBrains Mono");
/// Inner padding of the terminal area, in logical pixels.
pub const PADDING: f32 = 10.0;

/// Cell geometry derived from the font size. JetBrains Mono advance = 0.6 em.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub font_size: f32,
    pub cell_width: f32,
    pub cell_height: f32,
}

impl Metrics {
    pub fn new(font_size: f32) -> Self {
        Self {
            font_size,
            cell_width: font_size * 0.6,
            cell_height: (font_size * 1.35).ceil(),
        }
    }

    /// Grid (cols, rows) that fits in `size`.
    pub fn grid_for(&self, size: Size) -> (u16, u16) {
        let cols = ((size.width - 2.0 * PADDING) / self.cell_width)
            .floor()
            .max(2.0);
        let rows = ((size.height - 2.0 * PADDING) / self.cell_height)
            .floor()
            .max(1.0);
        (cols.min(1000.0) as u16, rows.min(500.0) as u16)
    }
}

pub struct TerminalCanvas<'a> {
    pub id: TermId,
    pub snapshot: &'a Snapshot,
    pub cache: &'a Cache,
    pub metrics: Metrics,
    pub modes: Modes,
    pub grid: (u16, u16),
    /// Keyboard focus: unfocused terminals draw a hollow cursor.
    pub focused: bool,
}

#[derive(Default)]
pub struct Interaction {
    selecting: bool,
    reported_button: Option<MouseButton>,
    last_cell: Option<(usize, usize)>,
}

impl TerminalCanvas<'_> {
    fn msg(&self, m: TermMsg) -> Message {
        Message::Term(self.id, m)
    }

    fn input(&self, bytes: Vec<u8>, focus: bool) -> Message {
        self.msg(TermMsg::Input { bytes, focus })
    }

    /// Cell under the pointer, plus whether it is on the right half of the cell.
    fn cell_at(&self, bounds: Rectangle, cursor: Cursor) -> Option<(usize, usize, bool)> {
        let p = cursor.position_in(bounds)?;
        let x = (p.x - PADDING).max(0.0) / self.metrics.cell_width;
        let y = (p.y - PADDING).max(0.0) / self.metrics.cell_height;
        let col = (x as usize).min(self.snapshot.cols.saturating_sub(1));
        let row = (y as usize).min(self.snapshot.rows.saturating_sub(1));
        Some((col, row, x.fract() >= 0.5))
    }

    fn report(
        &self,
        button: MouseButton,
        pressed: bool,
        col: usize,
        row: usize,
        mods: iced::keyboard::Modifiers,
    ) -> Option<Message> {
        encode_mouse(
            button,
            pressed,
            col,
            row,
            crate::keys::modifiers(mods),
            self.modes.sgr_mouse,
        )
        .map(|bytes| self.input(bytes, pressed && button != MouseButton::Motion))
    }
}

impl canvas::Program<Message> for TerminalCanvas<'_> {
    type State = Interaction;

    fn update(
        &self,
        state: &mut Interaction,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Message>> {
        match event {
            // Every redraw: make sure the PTY size matches the real widget size.
            Event::Window(window::Event::RedrawRequested(_) | window::Event::Resized(_)) => {
                let grid = self.metrics.grid_for(bounds.size());
                (grid != self.grid)
                    .then(|| Action::publish(self.msg(TermMsg::GridResized(grid.0, grid.1))))
            }
            Event::Mouse(mouse_event) => {
                let mods = iced::keyboard::Modifiers::default();
                match mouse_event {
                    mouse::Event::WheelScrolled { delta } => {
                        let (col, row, _) = self.cell_at(bounds, cursor)?;
                        let lines = match delta {
                            ScrollDelta::Lines { y, .. } => (y * 3.0).round() as i32,
                            ScrollDelta::Pixels { y, .. } => {
                                (y / self.metrics.cell_height).round() as i32
                            }
                        };
                        if lines == 0 {
                            return Some(Action::capture());
                        }
                        let up = lines > 0;
                        let msg = if self.modes.mouse_reporting {
                            let button = if up {
                                MouseButton::WheelUp
                            } else {
                                MouseButton::WheelDown
                            };
                            let one = encode_mouse(
                                button,
                                true,
                                col,
                                row,
                                Default::default(),
                                self.modes.sgr_mouse,
                            )?;
                            self.input(one.repeat(lines.unsigned_abs() as usize), false)
                        } else if self.modes.alt_screen && self.modes.alternate_scroll {
                            // Programs like `less` without mouse mode: translate to arrow keys.
                            let arrow: &[u8] = if self.modes.app_cursor {
                                if up { b"\x1bOA" } else { b"\x1bOB" }
                            } else if up {
                                b"\x1b[A"
                            } else {
                                b"\x1b[B"
                            };
                            self.input(arrow.repeat(lines.unsigned_abs() as usize), false)
                        } else {
                            self.msg(TermMsg::Scroll(lines))
                        };
                        Some(Action::publish(msg).and_capture())
                    }
                    mouse::Event::ButtonPressed(button) => {
                        let (col, row, right) = self.cell_at(bounds, cursor)?;
                        let tb = match button {
                            mouse::Button::Left => MouseButton::Left,
                            mouse::Button::Middle => MouseButton::Middle,
                            mouse::Button::Right => MouseButton::Right,
                            _ => return None,
                        };
                        if self.modes.mouse_reporting {
                            state.reported_button = Some(tb);
                            return self
                                .report(tb, true, col, row, mods)
                                .map(|m| Action::publish(m).and_capture());
                        }
                        match tb {
                            MouseButton::Left => {
                                state.selecting = true;
                                Some(
                                    Action::publish(
                                        self.msg(TermMsg::SelectStart(col, row, right)),
                                    )
                                    .and_capture(),
                                )
                            }
                            // Right click pastes, like Windows Terminal / PuTTY.
                            MouseButton::Right => {
                                Some(Action::publish(self.msg(TermMsg::Paste)).and_capture())
                            }
                            _ => None,
                        }
                    }
                    mouse::Event::CursorMoved { .. } => {
                        let (col, row, right) = self.cell_at(bounds, cursor)?;
                        if state.selecting {
                            return Some(
                                Action::publish(self.msg(TermMsg::SelectUpdate(col, row, right)))
                                    .and_capture(),
                            );
                        }
                        if self.modes.mouse_motion
                            && state.reported_button.is_some()
                            && state.last_cell != Some((col, row))
                        {
                            state.last_cell = Some((col, row));
                            return self
                                .report(MouseButton::Motion, true, col, row, mods)
                                .map(|m| Action::publish(m).and_capture());
                        }
                        None
                    }
                    mouse::Event::ButtonReleased(_) => {
                        if state.selecting {
                            state.selecting = false;
                            return Some(
                                Action::publish(self.msg(TermMsg::SelectEnd)).and_capture(),
                            );
                        }
                        if let Some(button) = state.reported_button.take() {
                            let (col, row, _) =
                                self.cell_at(bounds, cursor).unwrap_or((0, 0, false));
                            return self
                                .report(button, false, col, row, mods)
                                .map(|m| Action::publish(m).and_capture());
                        }
                        None
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Interaction,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let geometry = self.cache.draw(renderer, bounds.size(), |frame| {
            draw_snapshot(
                frame,
                self.snapshot,
                self.metrics,
                bounds.size(),
                self.focused,
            );
        });
        vec![geometry]
    }

    fn mouse_interaction(
        &self,
        _state: &Interaction,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) && !self.modes.mouse_reporting {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}

fn font_for(cell: &CellView) -> Font {
    Font {
        weight: if cell.flags.bold {
            Weight::Bold
        } else {
            Weight::Normal
        },
        style: if cell.flags.italic {
            Style::Italic
        } else {
            Style::Normal
        },
        ..TERMINAL_FONT
    }
}

fn draw_snapshot(frame: &mut Frame, snap: &Snapshot, m: Metrics, size: Size, focused: bool) {
    let (cw, ch) = (m.cell_width, m.cell_height);
    frame.fill_rectangle(Point::ORIGIN, size, to_color(snap.background));

    let origin =
        |col: usize, row: usize| Point::new(PADDING + col as f32 * cw, PADDING + row as f32 * ch);
    let text = |content: String, at: Point, color, font| Text {
        content,
        position: at,
        color,
        size: Pixels(m.font_size),
        line_height: LineHeight::Absolute(Pixels(ch)),
        font,
        align_y: Vertical::Top,
        ..Text::default()
    };

    for row in 0..snap.rows {
        let cells = snap.row(row);

        // Backgrounds: merge runs of equal non-default color.
        let mut col = 0;
        while col < cells.len() {
            let bg = cells[col].bg;
            let start = col;
            while col < cells.len() && cells[col].bg == bg {
                col += 1;
            }
            if bg != snap.background {
                frame.fill_rectangle(
                    origin(start, row),
                    Size::new((col - start) as f32 * cw, ch),
                    to_color(bg),
                );
            }
        }

        // Glyphs: ASCII runs with identical style are drawn as one string (monospace keeps
        // alignment); anything else is drawn per cell at its exact grid position.
        let mut col = 0;
        while col < cells.len() {
            let cell = &cells[col];
            if cell.flags.wide_spacer || (cell.c == ' ' && cell.zerowidth.is_none()) {
                col += 1;
                continue;
            }
            if glyphs::is_box_or_block(cell.c)
                && glyphs::draw(
                    frame,
                    cell.c,
                    origin(col, row),
                    Size::new(cw, ch),
                    to_color(cell.fg),
                )
            {
                col += 1;
                continue;
            }
            let font = font_for(cell);
            if cell.c.is_ascii_graphic() && cell.zerowidth.is_none() {
                let start = col;
                let mut run = String::new();
                while col < cells.len() {
                    let c = &cells[col];
                    let same = c.fg == cell.fg
                        && c.flags.bold == cell.flags.bold
                        && c.flags.italic == cell.flags.italic;
                    if !same || !(c.c.is_ascii_graphic() || c.c == ' ') || c.zerowidth.is_some() {
                        break;
                    }
                    run.push(c.c);
                    col += 1;
                }
                frame.fill_text(text(
                    run.trim_end().to_owned(),
                    origin(start, row),
                    to_color(cell.fg),
                    font,
                ));
            } else {
                let mut s = cell.c.to_string();
                if let Some(z) = &cell.zerowidth {
                    s.push_str(z);
                }
                frame.fill_text(text(s, origin(col, row), to_color(cell.fg), font));
                col += 1;
            }
        }

        // Decorations.
        for (col, cell) in cells.iter().enumerate() {
            let width = if cell.flags.wide { 2.0 * cw } else { cw };
            if cell.flags.underline {
                let p = origin(col, row);
                frame.fill_rectangle(
                    Point::new(p.x, p.y + ch - 2.0),
                    Size::new(width, 1.0),
                    to_color(cell.fg),
                );
            }
            if cell.flags.strikeout {
                let p = origin(col, row);
                frame.fill_rectangle(
                    Point::new(p.x, p.y + ch / 2.0),
                    Size::new(width, 1.0),
                    to_color(cell.fg),
                );
            }
        }
    }

    if let Some(cursor) = snap.cursor {
        let p = origin(cursor.col, cursor.row);
        let color = to_color(cursor.color);
        let cell = &snap.row(cursor.row)[cursor.col];
        let width = if cell.flags.wide { 2.0 * cw } else { cw };
        let shape = if focused {
            cursor.shape
        } else {
            CursorShape::HollowBlock
        };
        match shape {
            CursorShape::Block => {
                frame.fill_rectangle(p, Size::new(width, ch), color);
                if cell.c != ' ' {
                    frame.fill_text(text(
                        cell.c.to_string(),
                        p,
                        to_color(snap.background),
                        font_for(cell),
                    ));
                }
            }
            CursorShape::Beam => frame.fill_rectangle(p, Size::new(2.0, ch), color),
            CursorShape::Underline => frame.fill_rectangle(
                Point::new(p.x, p.y + ch - 2.0),
                Size::new(width, 2.0),
                color,
            ),
            CursorShape::HollowBlock => {
                // Four 1px edges: crisp without anti-aliasing.
                let (x, y) = (p.x.round(), p.y.round());
                let (w, h) = (width.round(), ch.round());
                frame.fill_rectangle(Point::new(x, y), Size::new(w, 1.0), color);
                frame.fill_rectangle(Point::new(x, y + h - 1.0), Size::new(w, 1.0), color);
                frame.fill_rectangle(Point::new(x, y), Size::new(1.0, h), color);
                frame.fill_rectangle(Point::new(x + w - 1.0, y), Size::new(1.0, h), color);
            }
        }
    }
}
