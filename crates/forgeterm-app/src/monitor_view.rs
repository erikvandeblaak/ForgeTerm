//! Monitor panel: measured CPU and memory of the machine and of each terminal's
//! process tree (forgeterm-system). Samples only while the panel is open; nothing
//! leaves the computer. GPU is not measured and says so.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use forgeterm_system::{Sample, Sampler};
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke, Text};
use iced::widget::{column, container, row, scrollable, space, text};
use iced::{Border, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

use crate::app::Message;
use crate::theme;

pub const WIDTH: f32 = 300.0;
/// Seconds between samples (the settings tick).
pub const INTERVAL_SECS: u32 = 2;
/// Samples kept for the sparklines: 3 minutes.
pub const HISTORY: usize = 90;

pub struct Monitor {
    pub visible: bool,
    pub sampler: Arc<Mutex<Sampler>>,
    pub busy: bool,
    pub last: Option<Sample>,
    pub cpu: VecDeque<f32>,
    pub memory: VecDeque<f32>,
}

impl Default for Monitor {
    fn default() -> Self {
        Self {
            visible: false,
            sampler: Arc::new(Mutex::new(Sampler::new())),
            busy: false,
            last: None,
            cpu: VecDeque::new(),
            memory: VecDeque::new(),
        }
    }
}

impl Monitor {
    pub fn record(&mut self, sample: Sample) {
        if let Some(c) = sample.cpu_percent {
            push(&mut self.cpu, c);
        }
        if sample.memory_total > 0 {
            push(
                &mut self.memory,
                sample.memory_used as f32 / sample.memory_total as f32 * 100.0,
            );
        }
        self.last = Some(sample);
        self.busy = false;
    }
}

fn push(history: &mut VecDeque<f32>, value: f32) {
    history.push_back(value);
    while history.len() > HISTORY {
        history.pop_front();
    }
}

pub fn bytes(n: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let n = n as f64;
    if n >= GB {
        format!("{:.1} GB", n / GB)
    } else {
        format!("{:.0} MB", n / MB)
    }
}

/// One row per terminal: (label, shell pid).
pub fn view(m: &Monitor, terminals: Vec<(String, u32)>) -> Element<'_, Message> {
    let mut body = column![].spacing(10);
    body = body.push(section("SYSTEM"));
    match &m.last {
        None => body = body.push(muted("Measuring…")),
        Some(s) => {
            let cpu = s
                .cpu_percent
                .map_or_else(|| "measuring…".to_owned(), |c| format!("{c:.0} %"));
            body = body.push(metric(
                format!("CPU · {} logical cores", s.logical_cpus),
                cpu,
                &m.cpu,
            ));
            let pct = if s.memory_total > 0 {
                s.memory_used as f64 / s.memory_total as f64 * 100.0
            } else {
                0.0
            };
            body = body.push(metric(
                "Memory".to_owned(),
                format!(
                    "{} / {} ({pct:.0} %)",
                    bytes(s.memory_used),
                    bytes(s.memory_total)
                ),
                &m.memory,
            ));
            body = body.push(
                row![
                    text("GPU").size(12).color(theme::TEXT),
                    space::horizontal(),
                    text("not measured").size(12).color(theme::TEXT_MUTED),
                ]
                .align_y(iced::Alignment::Center),
            );

            body = body.push(section("TERMINALS"));
            let mut any = false;
            for (label, pid) in terminals {
                let Some(t) = s.shells.get(&pid) else {
                    continue;
                };
                any = true;
                let cpu = t
                    .cpu_percent
                    .map_or_else(|| "…".to_owned(), |c| format!("{c:.1} %"));
                let mut col = column![
                    row![
                        text(label)
                            .size(12)
                            .color(theme::TEXT)
                            .wrapping(text::Wrapping::None),
                        space::horizontal(),
                        text(format!("{cpu} · {}", bytes(t.memory)))
                            .size(12)
                            .color(theme::CYAN),
                    ]
                    .align_y(iced::Alignment::Center),
                    text(format!(
                        "{} process{}{}",
                        t.processes,
                        if t.processes == 1 { "" } else { "es" },
                        if t.programs.is_empty() {
                            " · waiting at the prompt".to_owned()
                        } else {
                            format!(" · {}", t.programs.join(", "))
                        }
                    ))
                    .size(10)
                    .color(theme::TEXT_MUTED),
                ]
                .spacing(1);
                col = col.padding([2, 0]);
                body = body.push(col);
            }
            if !any {
                body = body.push(muted("No running terminals."));
            }
        }
    }
    body = body.push(muted(
        "Measured by the operating system every 2 s while this panel is open. Nothing leaves this computer.",
    ));

    container(
        column![
            text("MONITOR").size(11).color(theme::TEXT_MUTED),
            scrollable(body.padding(iced::Padding {
                top: 0.0,
                right: 14.0,
                bottom: 0.0,
                left: 8.0,
            }))
            .height(Length::Fill),
        ]
        .spacing(8),
    )
    .padding([10, 10])
    .width(Length::Fixed(WIDTH))
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

fn section(title: &str) -> Element<'_, Message> {
    text(title).size(11).color(theme::TEXT_MUTED).into()
}

fn muted(s: &str) -> Element<'_, Message> {
    text(s).size(11).color(theme::TEXT_MUTED).into()
}

fn metric<'a>(title: String, value: String, history: &'a VecDeque<f32>) -> Element<'a, Message> {
    column![
        row![
            text(title).size(12).color(theme::TEXT),
            space::horizontal(),
            text(value).size(12).color(theme::TEXT),
        ]
        .align_y(iced::Alignment::Center),
        Canvas::new(Sparkline { history })
            .width(Length::Fill)
            .height(46),
    ]
    .spacing(4)
    .into()
}

/// 0–100 % over the last [`HISTORY`] samples; newest at the right. Hover shows the
/// value and its age.
struct Sparkline<'a> {
    history: &'a VecDeque<f32>,
}

impl canvas::Program<Message> for Sparkline<'_> {
    /// The pointer was over the chart at the last event.
    type State = bool;

    /// Mouse movement alone does not redraw a canvas: ask for it while the pointer
    /// is over the chart (and once when it leaves) so the hover readout follows.
    fn update(
        &self,
        hovering: &mut bool,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<canvas::Action<Message>> {
        let canvas::Event::Mouse(iced::mouse::Event::CursorMoved { .. }) = event else {
            return None;
        };
        let over = cursor.is_over(bounds);
        let changed = over || *hovering;
        *hovering = over;
        changed.then(canvas::Action::request_redraw)
    }

    fn draw(
        &self,
        _state: &bool,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), theme::OBSIDIAN);
        // Recessive grid: 50 % line and the baseline.
        frame.fill_rectangle(
            Point::new(0.0, h / 2.0),
            Size::new(w, 1.0),
            Color {
                a: 0.5,
                ..theme::BORDER
            },
        );
        frame.fill_rectangle(Point::new(0.0, h - 1.0), Size::new(w, 1.0), theme::BORDER);

        let n = self.history.len();
        let step = w / (HISTORY.saturating_sub(1).max(1)) as f32;
        let point = |i: usize, v: f32| {
            // Right-aligned: the newest sample is at the right edge.
            let x = w - (n - 1 - i) as f32 * step;
            let y = h - 1.0 - (v.clamp(0.0, 100.0) / 100.0) * (h - 3.0);
            Point::new(x, y)
        };
        if n >= 2 {
            let line = Path::new(|p| {
                for (i, v) in self.history.iter().enumerate() {
                    if i == 0 {
                        p.move_to(point(i, *v));
                    } else {
                        p.line_to(point(i, *v));
                    }
                }
            });
            let area = Path::new(|p| {
                p.move_to(Point::new(point(0, 0.0).x, h - 1.0));
                for (i, v) in self.history.iter().enumerate() {
                    p.line_to(point(i, *v));
                }
                p.line_to(Point::new(w, h - 1.0));
                p.close();
            });
            frame.fill(
                &area,
                Color {
                    a: 0.15,
                    ..theme::CHART_CYAN
                },
            );
            frame.stroke(
                &line,
                Stroke::default()
                    .with_color(theme::CHART_CYAN)
                    .with_width(2.0),
            );
        }

        // Hover: crosshair + value of the nearest sample and how long ago.
        if let Some(p) = cursor.position_in(bounds)
            && n > 0
        {
            let from_right = ((w - p.x) / step).round().max(0.0) as usize;
            if from_right < n {
                let i = n - 1 - from_right;
                let at = point(i, self.history[i]);
                frame.fill_rectangle(
                    Point::new(at.x, 0.0),
                    Size::new(1.0, h),
                    Color {
                        a: 0.6,
                        ..theme::TEXT_MUTED
                    },
                );
                frame.fill(&Path::circle(at, 3.0), theme::CHART_CYAN);
                let ago = from_right as u32 * INTERVAL_SECS;
                let label = if ago == 0 {
                    format!("{:.0} % now", self.history[i])
                } else {
                    format!("{:.0} % · {ago} s ago", self.history[i])
                };
                let x = if at.x > w / 2.0 {
                    4.0
                } else {
                    w - 4.0 - label.len() as f32 * 6.0
                };
                frame.fill_text(Text {
                    content: label,
                    position: Point::new(x.max(2.0), 2.0),
                    color: theme::TEXT,
                    size: iced::Pixels(11.0),
                    ..Text::default()
                });
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_bounded_and_skips_missing_cpu() {
        let mut m = Monitor::default();
        let sample = |cpu: Option<f32>| Sample {
            cpu_percent: cpu,
            memory_used: 25,
            memory_total: 100,
            logical_cpus: 4,
            shells: Default::default(),
        };
        m.record(sample(None));
        assert!(m.cpu.is_empty(), "the first sample has no CPU value");
        assert_eq!(m.memory, [25.0]);
        for i in 0..200 {
            m.record(sample(Some(i as f32 % 100.0)));
        }
        assert_eq!(m.cpu.len(), HISTORY);
        assert_eq!(m.memory.len(), HISTORY);
        assert_eq!(*m.cpu.back().unwrap(), 99.0);
        assert!(!m.busy);
    }

    #[test]
    fn formats_bytes() {
        assert_eq!(bytes(512 * 1024 * 1024), "512 MB");
        assert_eq!(bytes(3 * 1024 * 1024 * 1024 + 512 * 1024 * 1024), "3.5 GB");
    }
}
