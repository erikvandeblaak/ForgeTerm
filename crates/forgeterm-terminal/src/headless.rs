//! A terminal emulator without a PTY: bytes in, snapshots out. For measuring the
//! emulator and snapshot cost (benches) and for testing what a program's output
//! looks like, without starting a shell.

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::term::{self, Term};
use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

use crate::palette::Palette;
use crate::session::{GridSize, SearchOutcome, build_snapshot, search_in};
use crate::snapshot::Snapshot;

pub struct Headless {
    term: Term<VoidListener>,
    parser: Processor<StdSyncHandler>,
}

impl Headless {
    pub fn new(cols: u16, rows: u16, scrollback: usize) -> Self {
        let config = term::Config {
            scrolling_history: scrollback,
            kitty_keyboard: true,
            ..Default::default()
        };
        let size = GridSize::new(cols, rows, 8, 16);
        Self {
            term: Term::new(config, &size, VoidListener),
            parser: Processor::new(),
        }
    }

    /// Runs output through the emulator, as if the program had written it.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    /// Scrolls the view `lines` up (positive) or down.
    pub fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    pub fn snapshot(&self, palette: &Palette) -> Snapshot {
        build_snapshot(&self.term, palette, None)
    }

    /// Like [`crate::Session::search`].
    pub fn search(&mut self, query: &str, older: bool) -> SearchOutcome {
        search_in(&mut self.term, query, older)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_colors_and_cursor() {
        let p = Palette::forge_dark();
        let mut t = Headless::new(20, 4, 100);
        t.feed(b"hi \x1b[31mred\x1b[0m \x1b[7minv\x1b[0m");
        let s = t.snapshot(&p);
        assert_eq!((s.cols, s.rows), (20, 4));
        assert_eq!(s.row_text(0).trim_end(), "hi red inv");
        let red = &s.row(0)[3];
        assert_eq!(red.c, 'r');
        assert_ne!(red.fg, p.foreground);
        // Inverse swaps foreground and background.
        let inv = &s.row(0)[7];
        assert_eq!((inv.fg, inv.bg), (p.background, p.foreground));
        let cursor = s.cursor.expect("cursor shown");
        assert_eq!((cursor.row, cursor.col), (0, 10));
    }

    #[test]
    fn redrawing_the_same_frame_gives_an_equal_snapshot() {
        // What `Session::refresh` relies on to skip a repaint.
        let p = Palette::forge_dark();
        let mut t = Headless::new(30, 5, 100);
        let frame = b"[?1049h[H[38;2;200;100;50mstatus[0m[3;1Hbody";
        t.feed(frame);
        let first = t.snapshot(&p);
        t.feed(frame);
        assert_eq!(t.snapshot(&p), first);
        t.feed(b"[3;1HBODY");
        assert_ne!(t.snapshot(&p), first);
    }

    #[test]
    fn wide_chars_and_scrollback() {
        let p = Palette::forge_dark();
        let mut t = Headless::new(10, 3, 100);
        t.feed("漢x".as_bytes());
        let s = t.snapshot(&p);
        assert!(s.row(0)[0].flags.wide);
        assert!(s.row(0)[1].flags.wide_spacer);
        assert_eq!(s.row(0)[2].c, 'x');
        for i in 0..10 {
            t.feed(format!("\r\nline {i}").as_bytes());
        }
        let live = t.snapshot(&p);
        assert_eq!(live.row_text(2).trim_end(), "line 9");
        assert!(live.history >= 8);
        t.scroll(2);
        let back = t.snapshot(&p);
        assert_eq!(back.display_offset, 2);
        assert_eq!(back.row_text(2).trim_end(), "line 7");
    }
}
