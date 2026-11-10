//! Code viewer panel: file tabs, a canvas with line numbers, highlighted text,
//! line selection and a minimap ("code glance"), plus find. Read-only.

use std::path::{Path, PathBuf};

use forgeterm_viewer::{Document, Span};
use iced::alignment::Vertical;
use iced::mouse::{self, Cursor, ScrollDelta};
use iced::widget::canvas::{self, Action, Cache, Canvas, Event, Frame, Geometry, Text};
use iced::widget::text::LineHeight;
use iced::widget::{Id, button, column, container, row, scrollable, text, text_input};
use iced::{
    Border, Color, Element, Length, Pixels, Point, Rectangle, Renderer, Size, Theme, window,
};

use crate::app::Message;
use crate::terminal_view::TERMINAL_FONT;
use crate::theme;

pub const FIND_INPUT_ID: &str = "forgeterm-viewer-find";
const PAD: f32 = 8.0;
const MINIMAP_WIDTH: f32 = 84.0;
/// Height of one line in the minimap, when the file fits.
const MINIMAP_LINE: f32 = 2.0;
/// Width of one column in the minimap.
const MINIMAP_COL: f32 = 0.75;

#[derive(Debug, Clone)]
pub enum ViewerMsg {
    /// Load a file into a tab (or focus its tab).
    Open(PathBuf),
    Loaded(PathBuf, Result<Box<Document>, String>),
    /// Highlighting finished for the version modified at the given time.
    Highlighted(
        PathBuf,
        Option<std::time::SystemTime>,
        Option<Vec<Vec<Span>>>,
    ),
    Scroll(i32),
    /// Center the view on a line (minimap click).
    CenterOn(usize),
    /// Minimap drag: scroll to this fraction (0..=1) of the file, like a scrollbar.
    ScrollFraction(f32),
    /// Visible text rows, reported by the canvas after layout.
    Rows(usize),
    /// Click on a line; `true` = Shift held: extend the selection.
    SelectStart(usize, bool),
    SelectTo(usize),
    SelectTab(usize),
    CloseTab(usize),
    FindQuery(String),
    FindNext,
    FindPrev,
    OpenFind,
    CloseFind,
    CopySelection,
    CopyPath,
    InsertPath,
    Reload,
    ClosePanel,
    /// Esc: clears the selection, then gives the keyboard back.
    Escape,
    SelectAll,
}

pub struct ViewerTab {
    pub doc: Document,
    /// A reload after an on-disk change is running.
    pub reloading: bool,
    pub spans: Option<Vec<Vec<Span>>>,
    pub highlighting: bool,
    /// First visible line.
    pub top: usize,
    pub rows: usize,
    /// (anchor, cursor) line numbers, 0-based.
    pub selection: Option<(usize, usize)>,
    pub cache: Cache,
}

impl ViewerTab {
    pub fn new(doc: Document) -> Self {
        Self {
            doc,
            reloading: false,
            spans: None,
            highlighting: false,
            top: 0,
            rows: 30,
            selection: None,
            cache: Cache::new(),
        }
    }

    fn max_top(&self) -> usize {
        self.doc.line_count().saturating_sub(self.rows.max(1))
    }

    pub fn scroll_by(&mut self, lines: i32) {
        let top = self.top as i64 - i64::from(lines);
        self.top = top.clamp(0, self.max_top() as i64) as usize;
        self.cache.clear();
    }

    pub fn scroll_to_fraction(&mut self, fraction: f32) {
        self.top = (fraction.clamp(0.0, 1.0) * self.max_top() as f32).round() as usize;
        self.cache.clear();
    }

    pub fn center_on(&mut self, line: usize) {
        self.top = line.saturating_sub(self.rows / 2).min(self.max_top());
        self.cache.clear();
    }

    /// Scrolls just enough to show `line`.
    pub fn reveal(&mut self, line: usize) {
        if line < self.top {
            self.top = line;
        } else if line >= self.top + self.rows {
            self.top = (line + 1).saturating_sub(self.rows);
        }
        self.top = self.top.min(self.max_top());
        self.cache.clear();
    }

    pub fn selected_range(&self) -> Option<(usize, usize)> {
        self.selection.map(|(a, b)| (a.min(b), a.max(b)))
    }

    /// Replaces the document (reload), keeping the position where possible.
    pub fn replace(&mut self, doc: Document) {
        self.doc = doc;
        self.reloading = false;
        self.spans = None;
        self.highlighting = false;
        let last = self.doc.line_count().saturating_sub(1);
        self.selection = self.selection.filter(|(a, b)| *a <= last && *b <= last);
        self.top = self.top.min(self.max_top());
        self.cache.clear();
    }
}

#[derive(Default)]
pub struct Viewer {
    pub tabs: Vec<ViewerTab>,
    pub active: usize,
    pub visible: bool,
    /// The viewer has the keyboard (after a click in it).
    pub focused: bool,
    pub find_open: bool,
    pub query: String,
    /// Lines of the active tab matching `query`.
    pub matches: Vec<usize>,
    pub current_match: Option<usize>,
    /// Problem opening a file, shown in the panel.
    pub error: Option<String>,
}

impl Viewer {
    pub fn tab(&self) -> Option<&ViewerTab> {
        self.tabs.get(self.active)
    }

    pub fn tab_mut(&mut self) -> Option<&mut ViewerTab> {
        self.tabs.get_mut(self.active)
    }

    pub fn index_of(&self, path: &Path) -> Option<usize> {
        self.tabs.iter().position(|t| t.doc.path == path)
    }

    /// Recomputes find matches for the active tab and jumps to the first one at or
    /// after the current view.
    pub fn refresh_matches(&mut self) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            self.matches.clear();
            self.current_match = None;
            return;
        };
        self.matches = forgeterm_viewer::find(&tab.doc, &self.query);
        self.current_match = self
            .matches
            .iter()
            .position(|&l| l >= tab.top)
            .or((!self.matches.is_empty()).then_some(0));
        if let Some(i) = self.current_match {
            let line = self.matches[i];
            // Already visible: stay; otherwise bring it to the middle.
            if line < tab.top || line >= tab.top + tab.rows {
                tab.center_on(line);
            }
        }
        tab.cache.clear();
    }

    pub fn step_match(&mut self, forward: bool) {
        if self.matches.is_empty() {
            return;
        }
        let n = self.matches.len();
        let next = match self.current_match {
            None => 0,
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
        };
        self.current_match = Some(next);
        let line = self.matches[next];
        if let Some(tab) = self.tab_mut() {
            tab.center_on(line);
            tab.selection = Some((line, line));
        }
    }
}

pub fn view<'a>(v: &'a Viewer, font_size: f32) -> Element<'a, Message> {
    let msg = |m: ViewerMsg| Message::Viewer(m);

    // Tabs.
    let mut tabs = row![].spacing(4).align_y(iced::Alignment::Center);
    for (i, t) in v.tabs.iter().enumerate() {
        let active = i == v.active;
        let name = t.doc.path.file_name().map_or_else(
            || t.doc.path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let label = button(text(name).size(12))
            .on_press(msg(ViewerMsg::SelectTab(i)))
            .padding([3, 8])
            .style(move |_, status| tab_style(active, status));
        let close = button(text("×").size(12))
            .on_press(msg(ViewerMsg::CloseTab(i)))
            .padding([3, 6])
            .style(|_, status| ghost(status));
        tabs = tabs.push(row![label, close].spacing(0));
    }
    let header = row![
        scrollable(tabs)
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(2).scroller_width(2)
            ))
            .width(Length::Fill),
        small("Find", Some(msg(ViewerMsg::OpenFind))),
        small("Copy path", v.tab().map(|_| msg(ViewerMsg::CopyPath))),
        small("Insert path", v.tab().map(|_| msg(ViewerMsg::InsertPath))),
        small("Reload", v.tab().map(|_| msg(ViewerMsg::Reload))),
        small("×", Some(msg(ViewerMsg::ClosePanel))),
    ]
    .spacing(4)
    .align_y(iced::Alignment::Center);

    let mut body = column![header].spacing(6);
    if let Some(e) = &v.error {
        body = body.push(text(e.as_str()).size(12).color(theme::CORAL));
    }
    if v.find_open {
        let count = match (v.current_match, v.matches.len()) {
            (_, 0) if !v.query.is_empty() => "no matches".to_owned(),
            (_, 0) => String::new(),
            (Some(i), n) => format!("{} of {n} lines", i + 1),
            (None, n) => format!("{n} lines"),
        };
        body = body.push(
            row![
                text_input("Find in file", &v.query)
                    .id(Id::new(FIND_INPUT_ID))
                    .on_input(|q| Message::Viewer(ViewerMsg::FindQuery(q)))
                    .on_submit(msg(ViewerMsg::FindNext))
                    .size(12)
                    .padding([4, 8])
                    .width(Length::Fill)
                    .style(|_, status| text_input::Style {
                        background: theme::OBSIDIAN.into(),
                        border: Border {
                            color: if matches!(status, text_input::Status::Focused { .. }) {
                                theme::CYAN
                            } else {
                                theme::BORDER
                            },
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        icon: theme::TEXT_MUTED,
                        placeholder: theme::TEXT_MUTED,
                        value: theme::TEXT,
                        selection: theme::SELECTION,
                    }),
                text(count).size(11).color(theme::TEXT_MUTED),
                small("↑", Some(msg(ViewerMsg::FindPrev))),
                small("↓", Some(msg(ViewerMsg::FindNext))),
                small("×", Some(msg(ViewerMsg::CloseFind))),
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
        );
    }

    match v.tab() {
        None => {
            body = body.push(
                container(
                    text("Open a file from the explorer (View) or from a Git diff.")
                        .size(12)
                        .color(theme::TEXT_MUTED),
                )
                .center(Length::Fill),
            );
        }
        Some(tab) => {
            let canvas = Canvas::new(ViewerCanvas {
                tab,
                font_size,
                matches: if v.find_open { &v.matches } else { &[] },
                current: v
                    .current_match
                    .and_then(|i| v.matches.get(i).copied())
                    .filter(|_| v.find_open),
            })
            .width(Length::Fill)
            .height(Length::Fill);
            body = body.push(
                container(canvas)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(move |_| container::Style {
                        border: Border {
                            color: if v.focused {
                                theme::CYAN
                            } else {
                                theme::BORDER
                            },
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    })
                    .padding(1),
            );
            body = body.push(status_line(tab));
        }
    }

    container(body)
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(theme::GRAPHITE.into()),
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn status_line(tab: &ViewerTab) -> Element<'_, Message> {
    let d = &tab.doc;
    let size = if d.bytes >= 1024 * 1024 {
        format!("{:.1} MB", d.bytes as f64 / 1_048_576.0)
    } else if d.bytes >= 1024 {
        format!("{:.1} KB", d.bytes as f64 / 1024.0)
    } else {
        format!("{} B", d.bytes)
    };
    let colors = if tab.spans.is_some() {
        d.syntax.clone()
    } else if tab.highlighting {
        format!("{} (coloring…)", d.syntax)
    } else if d.syntax != "Plain Text" && !d.highlightable() {
        format!("{} (too large to color)", d.syntax)
    } else {
        d.syntax.clone()
    };
    let last = (tab.top + tab.rows).min(d.line_count());
    let mut parts = vec![
        colors,
        format!("{} lines", d.line_count()),
        d.encoding.label().to_owned(),
        if d.crlf { "CRLF" } else { "LF" }.to_owned(),
        size,
        format!("lines {}–{last}", (tab.top + 1).min(last)),
    ];
    if let Some((a, b)) = tab.selected_range() {
        parts.push(format!(
            "{} line{} selected (Ctrl+C)",
            b - a + 1,
            if a == b { "" } else { "s" }
        ));
    }
    text(parts.join(" · "))
        .size(11)
        .color(theme::TEXT_MUTED)
        .wrapping(text::Wrapping::None)
        .into()
}

struct ViewerCanvas<'a> {
    tab: &'a ViewerTab,
    font_size: f32,
    matches: &'a [usize],
    current: Option<usize>,
}

#[derive(Default)]
struct Interaction {
    selecting: bool,
    dragging_minimap: bool,
    shift: bool,
}

struct Layout {
    cw: f32,
    ch: f32,
    gutter: f32,
    minimap_x: f32,
    rows: usize,
}

impl ViewerCanvas<'_> {
    fn layout(&self, size: Size) -> Layout {
        let cw = self.font_size * 0.6;
        let ch = (self.font_size * 1.35).ceil();
        let digits = self.tab.doc.line_count().max(1).to_string().len() as f32;
        Layout {
            cw,
            ch,
            gutter: PAD + digits * cw + 12.0,
            minimap_x: (size.width - MINIMAP_WIDTH).max(0.0),
            rows: ((size.height - 2.0 * PAD) / ch).floor().max(1.0) as usize,
        }
    }

    fn line_at(&self, l: &Layout, y: f32) -> usize {
        let row = ((y - PAD) / l.ch).floor().max(0.0) as usize;
        (self.tab.top + row).min(self.tab.doc.line_count().saturating_sub(1))
    }

    /// First line drawn in the minimap and the per-line height there.
    fn minimap_window(&self, height: f32) -> (usize, f32) {
        let total = self.tab.doc.line_count();
        let fits = (height / MINIMAP_LINE).floor() as usize;
        if total <= fits {
            return (0, MINIMAP_LINE);
        }
        // Longer than the minimap: it scrolls along, proportionally.
        let max_top = self.tab.max_top().max(1);
        let frac = self.tab.top as f32 / max_top as f32;
        let start = (frac * (total - fits) as f32) as usize;
        (start, MINIMAP_LINE)
    }

    fn minimap_line_at(&self, height: f32, y: f32) -> usize {
        let (start, lh) = self.minimap_window(height);
        (start + (y / lh).max(0.0) as usize).min(self.tab.doc.line_count().saturating_sub(1))
    }
}

impl canvas::Program<Message> for ViewerCanvas<'_> {
    type State = Interaction;

    fn update(
        &self,
        state: &mut Interaction,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Message>> {
        let l = self.layout(bounds.size());
        let m = |v: ViewerMsg| Message::Viewer(v);
        match event {
            Event::Window(window::Event::RedrawRequested(_) | window::Event::Resized(_)) => {
                (l.rows != self.tab.rows).then(|| Action::publish(m(ViewerMsg::Rows(l.rows))))
            }
            Event::Keyboard(iced::keyboard::Event::ModifiersChanged(mods)) => {
                state.shift = mods.shift();
                None
            }
            Event::Mouse(e) => match e {
                mouse::Event::WheelScrolled { delta } => {
                    cursor.position_in(bounds)?;
                    let lines = match delta {
                        ScrollDelta::Lines { y, .. } => (y * 3.0).round() as i32,
                        ScrollDelta::Pixels { y, .. } => (y / l.ch).round() as i32,
                    };
                    (lines != 0).then(|| Action::publish(m(ViewerMsg::Scroll(lines))).and_capture())
                }
                mouse::Event::ButtonPressed(mouse::Button::Left) => {
                    let p = cursor.position_in(bounds)?;
                    if p.x >= l.minimap_x {
                        state.dragging_minimap = true;
                        let line = self.minimap_line_at(bounds.height, p.y);
                        return Some(Action::publish(m(ViewerMsg::CenterOn(line))).and_capture());
                    }
                    state.selecting = true;
                    let line = self.line_at(&l, p.y);
                    Some(
                        Action::publish(m(ViewerMsg::SelectStart(line, state.shift))).and_capture(),
                    )
                }
                mouse::Event::CursorMoved { .. } => {
                    if state.dragging_minimap {
                        // A fraction, not "the line under the pointer": the minimap of
                        // a long file scrolls along, which would feed back into the drag.
                        let p = cursor.position_from(bounds.position())?;
                        let fraction = (p.y / bounds.height.max(1.0)).clamp(0.0, 1.0);
                        return Some(
                            Action::publish(m(ViewerMsg::ScrollFraction(fraction))).and_capture(),
                        );
                    }
                    if state.selecting {
                        let p = cursor.position_from(bounds.position())?;
                        // Above / below the text: select (and scroll) one line further.
                        let line = if p.y < PAD {
                            self.tab.top.saturating_sub(1)
                        } else if p.y > bounds.height - PAD {
                            self.tab.top + self.tab.rows
                        } else {
                            self.line_at(&l, p.y)
                        };
                        return Some(Action::publish(m(ViewerMsg::SelectTo(line))).and_capture());
                    }
                    None
                }
                mouse::Event::ButtonReleased(mouse::Button::Left) => {
                    let was = state.selecting || state.dragging_minimap;
                    state.selecting = false;
                    state.dragging_minimap = false;
                    was.then(Action::capture)
                }
                _ => None,
            },
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
        let geometry = self.tab.cache.draw(renderer, bounds.size(), |frame| {
            self.draw_text(frame, bounds.size());
            self.draw_minimap(frame, bounds.size());
        });
        vec![geometry]
    }

    fn mouse_interaction(
        &self,
        _state: &Interaction,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> mouse::Interaction {
        match cursor.position_in(bounds) {
            Some(p) if p.x >= self.layout(bounds.size()).minimap_x => mouse::Interaction::Pointer,
            Some(_) => mouse::Interaction::Text,
            None => mouse::Interaction::default(),
        }
    }
}

fn to_color(c: forgeterm_viewer::Rgb) -> Color {
    Color::from_rgb8(c.r, c.g, c.b)
}

impl ViewerCanvas<'_> {
    fn draw_text(&self, frame: &mut Frame, size: Size) {
        let l = self.layout(size);
        frame.fill_rectangle(Point::ORIGIN, size, theme::OBSIDIAN);
        let doc = &self.tab.doc;
        let selected = self.tab.selected_range();
        let text_x = l.gutter;
        let max_cols = ((l.minimap_x - text_x - PAD) / l.cw).max(1.0) as usize;
        let font = |bold: bool, italic: bool| iced::Font {
            weight: if bold {
                iced::font::Weight::Bold
            } else {
                iced::font::Weight::Normal
            },
            style: if italic {
                iced::font::Style::Italic
            } else {
                iced::font::Style::Normal
            },
            ..TERMINAL_FONT
        };
        let label = |content: String, at: Point, color: Color, f: iced::Font| Text {
            content,
            position: at,
            color,
            size: Pixels(self.font_size),
            line_height: LineHeight::Absolute(Pixels(l.ch)),
            font: f,
            align_y: Vertical::Top,
            ..Text::default()
        };

        for row in 0..l.rows {
            let line = self.tab.top + row;
            let Some(display) = doc.display.get(line) else {
                break;
            };
            let y = PAD + row as f32 * l.ch;
            let is_selected = selected.is_some_and(|(a, b)| (a..=b).contains(&line));
            let is_match = self.matches.binary_search(&line).is_ok();
            if is_selected {
                frame.fill_rectangle(
                    Point::new(0.0, y),
                    Size::new(l.minimap_x, l.ch),
                    theme::SELECTION,
                );
            } else if is_match {
                let alpha = if self.current == Some(line) {
                    0.30
                } else {
                    0.14
                };
                frame.fill_rectangle(
                    Point::new(0.0, y),
                    Size::new(l.minimap_x, l.ch),
                    Color {
                        a: alpha,
                        ..theme::AMBER
                    },
                );
            }
            // Line number, right-aligned in the gutter.
            let number = (line + 1).to_string();
            let nx = l.gutter - 12.0 - number.len() as f32 * l.cw;
            frame.fill_text(label(
                number,
                Point::new(nx, y),
                // Muted numbers would vanish on the selection / match background.
                if is_selected || is_match {
                    theme::TEXT
                } else {
                    theme::DISABLED
                },
                TERMINAL_FONT,
            ));

            // Text: highlighted spans, or the plain line.
            let visible = |s: &str| -> String { s.chars().take(max_cols).collect() };
            match self.tab.spans.as_ref().and_then(|s| s.get(line)) {
                Some(spans) => {
                    for span in spans {
                        let Some(piece) = display.get(span.start..span.end) else {
                            continue;
                        };
                        let col = display[..span.start].chars().count();
                        if col >= max_cols {
                            break;
                        }
                        let piece: String = piece.chars().take(max_cols - col).collect();
                        frame.fill_text(label(
                            piece,
                            Point::new(text_x + col as f32 * l.cw, y),
                            to_color(span.color),
                            font(span.bold, span.italic),
                        ));
                    }
                }
                None => {
                    if !display.trim().is_empty() {
                        frame.fill_text(label(
                            visible(display),
                            Point::new(text_x, y),
                            theme::TEXT,
                            TERMINAL_FONT,
                        ));
                    }
                }
            }
        }
        // Gutter separator.
        frame.fill_rectangle(
            Point::new(l.gutter - 6.0, 0.0),
            Size::new(1.0, size.height),
            theme::BORDER,
        );
    }

    fn draw_minimap(&self, frame: &mut Frame, size: Size) {
        let l = self.layout(size);
        let x0 = l.minimap_x;
        frame.fill_rectangle(
            Point::new(x0, 0.0),
            Size::new(MINIMAP_WIDTH, size.height),
            theme::GRAPHITE,
        );
        frame.fill_rectangle(
            Point::new(x0, 0.0),
            Size::new(1.0, size.height),
            theme::BORDER,
        );
        let doc = &self.tab.doc;
        let (start, lh) = self.minimap_window(size.height);
        let visible_lines = (size.height / lh).ceil() as usize;
        let max_cols = ((MINIMAP_WIDTH - 8.0) / MINIMAP_COL) as usize;
        let bar = |frame: &mut Frame, line_row: usize, col: usize, len: usize, color: Color| {
            if col >= max_cols || len == 0 {
                return;
            }
            let len = len.min(max_cols - col);
            frame.fill_rectangle(
                Point::new(x0 + 5.0 + col as f32 * MINIMAP_COL, line_row as f32 * lh),
                Size::new(len as f32 * MINIMAP_COL, (lh - 0.5).max(1.0)),
                color,
            );
        };
        for row in 0..visible_lines {
            let line = start + row;
            let Some(display) = doc.display.get(line) else {
                break;
            };
            if self.matches.binary_search(&line).is_ok() {
                frame.fill_rectangle(
                    Point::new(x0 + 1.0, row as f32 * lh),
                    Size::new(MINIMAP_WIDTH - 1.0, lh),
                    Color {
                        a: 0.55,
                        ..theme::AMBER
                    },
                );
            }
            match self.tab.spans.as_ref().and_then(|s| s.get(line)) {
                Some(spans) => {
                    for span in spans {
                        let Some(piece) = display.get(span.start..span.end) else {
                            continue;
                        };
                        // Leading/trailing blanks of a span are not ink.
                        let lead = piece.chars().take_while(|c| *c == ' ').count();
                        let ink = piece.trim().chars().count();
                        let col = display[..span.start].chars().count() + lead;
                        bar(
                            frame,
                            row,
                            col,
                            ink,
                            Color {
                                a: 0.8,
                                ..to_color(span.color)
                            },
                        );
                    }
                }
                None => {
                    let indent = display.chars().take_while(|c| *c == ' ').count();
                    let len = display.trim_end().chars().count().saturating_sub(indent);
                    bar(
                        frame,
                        row,
                        indent,
                        len,
                        Color {
                            a: 0.45,
                            ..theme::TEXT
                        },
                    );
                }
            }
        }
        // The part of the file the text area shows.
        let top = self.tab.top.saturating_sub(start) as f32 * lh;
        let height = (self.tab.rows.min(doc.line_count()) as f32 * lh).max(4.0);
        frame.fill_rectangle(
            Point::new(x0 + 1.0, top),
            Size::new(MINIMAP_WIDTH - 1.0, height),
            Color {
                a: 0.10,
                ..theme::CYAN
            },
        );
        frame.stroke(
            &canvas::Path::rectangle(
                Point::new(x0 + 1.5, top + 0.5),
                Size::new(MINIMAP_WIDTH - 2.0, height - 1.0),
            ),
            canvas::Stroke::default()
                .with_color(Color {
                    a: 0.5,
                    ..theme::CYAN
                })
                .with_width(1.0),
        );
    }
}

fn tab_style(active: bool, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(
            if active {
                theme::ELEVATED
            } else {
                theme::GRAPHITE
            }
            .into(),
        ),
        text_color: if active || hovered {
            theme::TEXT
        } else {
            theme::TEXT_MUTED
        },
        border: Border {
            color: if active { theme::CYAN } else { theme::BORDER },
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

fn ghost(status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then(|| theme::ELEVATED.into()),
        text_color: if hovered {
            theme::CORAL
        } else {
            theme::TEXT_MUTED
        },
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn small(label: &str, on_press: Option<Message>) -> Element<'_, Message> {
    let enabled = on_press.is_some();
    button(text(label).size(11))
        .on_press_maybe(on_press)
        .padding([3, 8])
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(
                    if hovered {
                        theme::ELEVATED
                    } else {
                        theme::SURFACE
                    }
                    .into(),
                ),
                text_color: if !enabled {
                    theme::DISABLED
                } else if hovered {
                    theme::CYAN
                } else {
                    theme::TEXT
                },
                border: Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(lines: usize) -> Document {
        let text: Vec<String> = (0..lines).map(|i| format!("line {i}")).collect();
        Document {
            path: PathBuf::from("/x.txt"),
            display: text.clone(),
            lines: text,
            encoding: forgeterm_viewer::Encoding::Utf8,
            crlf: false,
            bytes: 10,
            modified: None,
            syntax: "Plain Text".into(),
        }
    }

    #[test]
    fn scrolling_is_clamped_to_the_document() {
        let mut t = ViewerTab::new(doc(100));
        t.rows = 30;
        t.scroll_by(-500);
        assert_eq!(t.top, 70);
        t.scroll_by(1000);
        assert_eq!(t.top, 0);
        t.center_on(50);
        assert_eq!(t.top, 35);
        t.center_on(99);
        assert_eq!(t.top, 70);
        t.reveal(10);
        assert_eq!(t.top, 10);
        t.reveal(45);
        assert_eq!(t.top, 16);
        // A short file never scrolls.
        let mut short = ViewerTab::new(doc(5));
        short.scroll_by(-10);
        assert_eq!(short.top, 0);
    }

    #[test]
    fn minimap_drag_scrolls_by_fraction() {
        let mut t = ViewerTab::new(doc(1000));
        t.rows = 40;
        t.scroll_to_fraction(0.5);
        assert_eq!(t.top, 480);
        t.scroll_to_fraction(1.5);
        assert_eq!(t.top, 960);
        t.scroll_to_fraction(-1.0);
        assert_eq!(t.top, 0);
    }

    #[test]
    fn find_centers_a_match_that_is_off_screen() {
        let mut v = Viewer::default();
        let mut t = ViewerTab::new(doc(1000));
        t.rows = 40;
        v.tabs.push(t);
        v.query = "line 900".into();
        v.refresh_matches();
        assert_eq!(v.tab().unwrap().top, 880);
        // Visible already: the view does not jump.
        v.query = "line 890".into();
        v.refresh_matches();
        assert_eq!(v.tab().unwrap().top, 880);
    }

    #[test]
    fn find_cycles_through_matching_lines() {
        let mut v = Viewer::default();
        v.tabs.push(ViewerTab::new(doc(30)));
        v.query = "line 1".into();
        v.refresh_matches();
        // "line 1", "line 10".."line 19"
        assert_eq!(v.matches.len(), 11);
        assert_eq!(v.current_match, Some(0));
        v.step_match(false);
        assert_eq!(v.current_match, Some(10));
        v.step_match(true);
        assert_eq!(v.current_match, Some(0));
        assert_eq!(v.tab().unwrap().selection, Some((1, 1)));
        v.query = "nothing".into();
        v.refresh_matches();
        assert!(v.matches.is_empty());
        assert_eq!(v.current_match, None);
        v.step_match(true);
    }

    #[test]
    fn reload_keeps_position_and_drops_invalid_selection() {
        let mut t = ViewerTab::new(doc(100));
        t.rows = 20;
        t.top = 60;
        t.selection = Some((90, 95));
        t.replace(doc(50));
        assert_eq!(t.top, 30);
        assert_eq!(t.selection, None);
    }
}
