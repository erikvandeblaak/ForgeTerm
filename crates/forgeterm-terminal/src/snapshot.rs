use crate::palette::Rgb;

/// Visual attributes of a cell that the renderer must honour.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CellFlags {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
    /// First half of a double-width glyph.
    pub wide: bool,
    /// Second (placeholder) half of a double-width glyph; draw nothing.
    pub wide_spacer: bool,
}

/// A fully resolved cell: colors already include inverse/dim/selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellView {
    pub c: char,
    /// Combining characters attached to `c`, if any.
    pub zerowidth: Option<Box<str>>,
    pub fg: Rgb,
    pub bg: Rgb,
    pub flags: CellFlags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Underline,
    Beam,
    HollowBlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorView {
    pub col: usize,
    pub row: usize,
    pub shape: CursorShape,
    pub color: Rgb,
}

/// How a marked prompt row is drawn in the margin (shell integration).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptMark {
    /// A prompt whose command result is unknown (shells that only mark prompts).
    Prompt,
    Running,
    Succeeded,
    /// Non-zero exit code.
    Failed(i32),
}

/// An immutable copy of the visible viewport, ready to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub cols: usize,
    pub rows: usize,
    /// Row-major, `rows * cols` entries.
    pub cells: Vec<CellView>,
    pub cursor: Option<CursorView>,
    pub background: Rgb,
    /// Lines scrolled back from the bottom (0 = live view).
    pub display_offset: usize,
    /// Total lines available in scrollback.
    pub history: usize,
    /// Viewport rows where a marked prompt starts.
    pub prompt_marks: Vec<(usize, PromptMark)>,
}

impl Snapshot {
    pub fn row(&self, row: usize) -> &[CellView] {
        &self.cells[row * self.cols..(row + 1) * self.cols]
    }

    /// Text of one row with trailing blanks trimmed.
    pub fn row_text(&self, row: usize) -> String {
        let mut s: String = self
            .row(row)
            .iter()
            .filter(|c| !c.flags.wide_spacer)
            .flat_map(|c| std::iter::once(c.c).chain(c.zerowidth.iter().flat_map(|z| z.chars())))
            .collect();
        s.truncate(s.trim_end().len());
        s
    }

    /// Whole viewport as text, one line per row.
    pub fn text(&self) -> String {
        (0..self.rows)
            .map(|r| self.row_text(r))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
