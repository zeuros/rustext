//! Styles as plain data, shared by highlighters and decorations (DECISIONS D11). The UI
//! crate turns them into its own colors and fonts.

use crate::text::Text;

/// An sRGB color with straight (not premultiplied) alpha.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgba {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl Rgba {
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue, alpha: 255 }
    }

    pub const fn with_alpha(self, alpha: u8) -> Self {
        Self { alpha, ..self }
    }

    /// `0xRRGGBB`.
    pub const fn hex(rgb: u32) -> Self {
        Self::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextStyle {
    pub foreground: Option<Rgba>,
    pub background: Option<Rgba>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

/// A styled run of chars inside one line, in char columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StyledSpan {
    pub start_column: usize,
    pub end_column: usize,
    pub style: TextStyle,
}

/// Something that styles lines: a syntax highlighter. It may cache per line; `invalidate_from`
/// tells it the text changed from that line on.
pub trait LineStyler {
    /// The spans of `line`, sorted, not overlapping; chars outside spans use the default color.
    fn style_line(&mut self, text: &Text, line: usize) -> Vec<StyledSpan>;

    /// The text changed at `first_changed_line` and after.
    fn invalidate_from(&mut self, first_changed_line: usize);

    /// An edit replaced some lines (D19): a styler keeping state per line can shift what
    /// follows instead of dropping it. Defaults to `invalidate_from`.
    fn lines_edited(&mut self, edited: &crate::state::EditedLines) {
        self.invalidate_from(edited.first_line);
    }

    /// Whether some lines were left unstyled to keep the frame fast: paint again soon.
    fn is_catching_up(&self) -> bool {
        false
    }
}
