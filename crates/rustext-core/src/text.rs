//! The document text: an immutable value backed by a rope (DECISIONS D4, D5).

use std::fmt;

use ropey::Rope;

use crate::position::{Position, Range};

/// Which line ending a text uses, detected from its first line break.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineEnding {
    #[default]
    Lf,
    CrLf,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::CrLf => "\r\n",
        }
    }
}

/// Text split in lines on `\n` only. A `\r` before a `\n` is kept in the text but is not part of
/// the line's content: columns stop before it.
///
/// Cloning is O(1) (structural sharing), so a `Text` is passed around and kept as a value:
/// edits return a new `Text` and older versions stay valid.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Text {
    rope: Rope,
}

impl Text {
    pub fn new(content: &str) -> Self {
        Self { rope: Rope::from_str(content) }
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn is_empty(&self) -> bool {
        self.rope.len_chars() == 0
    }

    /// Number of lines: one more than the number of `\n`.
    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn last_line(&self) -> usize {
        self.line_count() - 1
    }

    /// Char offset where `line` starts. Lines past the end start at the end of the text.
    pub fn line_start(&self, line: usize) -> usize {
        if line >= self.line_count() {
            return self.len_chars();
        }
        self.rope.line_to_char(line)
    }

    /// Char offset where the content of `line` ends: before its `\r\n` or `\n`.
    pub fn line_content_end(&self, line: usize) -> usize {
        if line >= self.last_line() {
            return self.len_chars();
        }
        let newline_offset = self.rope.line_to_char(line + 1) - 1;
        let has_carriage_return = newline_offset > self.line_start(line) && self.rope.char(newline_offset - 1) == '\r';
        if has_carriage_return { newline_offset - 1 } else { newline_offset }
    }

    /// Length of the line's content in chars, line ending excluded.
    pub fn line_len(&self, line: usize) -> usize {
        self.line_content_end(line) - self.line_start(line)
    }

    /// The line's content, line ending excluded.
    pub fn line_content(&self, line: usize) -> String {
        self.slice(self.line_start(line), self.line_content_end(line))
    }

    /// The line ending after `line`: `None` for the last line.
    pub fn line_ending_of(&self, line: usize) -> Option<LineEnding> {
        if line >= self.last_line() {
            return None;
        }
        let content_end = self.line_content_end(line);
        Some(if self.rope.char(content_end) == '\r' { LineEnding::CrLf } else { LineEnding::Lf })
    }

    /// The line ending of the first line break, `Lf` when there is none.
    pub fn line_ending(&self) -> LineEnding {
        self.line_ending_of(0).unwrap_or_default()
    }

    /// Line holding `offset`. An offset between `\r` and `\n` belongs to the line they end.
    pub fn line_of_offset(&self, offset: usize) -> usize {
        self.rope.char_to_line(offset.min(self.len_chars()))
    }

    /// Char offset of a position, clamped: past the last line goes to the end of the text,
    /// past the end of a line goes to the end of its content.
    pub fn offset(&self, position: Position) -> usize {
        if position.line >= self.line_count() {
            return self.len_chars();
        }
        self.line_start(position.line) + position.column.min(self.line_len(position.line))
    }

    /// Position of a char offset, clamped to the text and to the line's content.
    pub fn position(&self, offset: usize) -> Position {
        let line = self.line_of_offset(offset);
        let column = offset.min(self.line_content_end(line)) - self.line_start(line);
        Position::new(line, column)
    }

    /// Clamps an offset so it never falls inside a line ending.
    pub fn clamp_offset(&self, offset: usize) -> usize {
        self.offset(self.position(offset))
    }

    pub fn range_offsets(&self, range: Range) -> (usize, usize) {
        (self.offset(range.start), self.offset(range.end))
    }

    pub fn slice(&self, start: usize, end: usize) -> String {
        let end = end.min(self.len_chars());
        self.rope.slice(start.min(end)..end).to_string()
    }

    pub fn slice_range(&self, range: Range) -> String {
        let (start, end) = self.range_offsets(range);
        self.slice(start, end)
    }

    pub fn char_at(&self, offset: usize) -> Option<char> {
        (offset < self.len_chars()).then(|| self.rope.char(offset))
    }

    /// Chars from `offset` to the end of the text.
    pub fn chars_from(&self, offset: usize) -> impl Iterator<Item = char> + '_ {
        self.rope.chars_at(offset.min(self.len_chars()))
    }

    /// Chars before `offset`, nearest first.
    pub fn chars_before(&self, offset: usize) -> impl Iterator<Item = char> + '_ {
        let mut chars = self.rope.chars_at(offset.min(self.len_chars()));
        std::iter::from_fn(move || chars.prev())
    }

    /// Replaces the chars in `start..end` with `insert`, returning the new text.
    pub(crate) fn replaced(&self, replacements: impl Iterator<Item = (usize, usize, String)>) -> Text {
        let mut rope = self.rope.clone();
        for (start, end, insert) in replacements {
            rope.remove(start..end);
            rope.insert(start, &insert);
        }
        Text { rope }
    }

    pub fn lines(&self) -> impl Iterator<Item = String> + '_ {
        (0..self.line_count()).map(|line| self.line_content(line))
    }

    /// Every line with its line ending (`\n`, `\r\n`, or none for the last line), read in one
    /// sequential pass: much faster than `line_content` per line for whole-text work.
    pub fn raw_lines(&self) -> impl Iterator<Item = String> + '_ {
        self.rope.lines().map(String::from)
    }
}

impl fmt::Display for Text {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for chunk in self.rope.chunks() {
            formatter.write_str(chunk)?;
        }
        Ok(())
    }
}

impl fmt::Debug for Text {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Text({:?})", self.to_string())
    }
}

impl From<&str> for Text {
    fn from(content: &str) -> Self {
        Text::new(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_split_on_newline_only() {
        let text = Text::new("a\r\nbc\ndef\u{2028}g");

        assert_eq!(text.line_count(), 3);
        assert_eq!(text.line_content(0), "a");
        assert_eq!(text.line_content(1), "bc");
        assert_eq!(text.line_content(2), "def\u{2028}g");
        assert_eq!(text.line_ending_of(0), Some(LineEnding::CrLf));
        assert_eq!(text.line_ending_of(1), Some(LineEnding::Lf));
        assert_eq!(text.line_ending_of(2), None);
        assert_eq!(text.line_ending(), LineEnding::CrLf);
    }

    #[test]
    fn positions_never_point_inside_a_line_ending() {
        let text = Text::new("ab\r\ncd");

        assert_eq!(text.position(2), Position::new(0, 2));
        assert_eq!(text.position(3), Position::new(0, 2));
        assert_eq!(text.position(4), Position::new(1, 0));
        assert_eq!(text.offset(Position::new(0, 99)), 2);
        assert_eq!(text.offset(Position::new(9, 0)), 6);
        assert_eq!(text.clamp_offset(3), 2);
    }

    #[test]
    fn empty_text_has_one_empty_line() {
        let text = Text::new("");

        assert_eq!(text.line_count(), 1);
        assert_eq!(text.line_content(0), "");
        assert_eq!(text.position(0), Position::new(0, 0));
    }

    #[test]
    fn trailing_newline_makes_an_empty_last_line() {
        let text = Text::new("a\n");

        assert_eq!(text.line_count(), 2);
        assert_eq!(text.line_content(1), "");
        assert_eq!(text.line_start(1), 2);
    }

    #[test]
    fn chars_before_goes_backwards() {
        let text = Text::new("abc");

        assert_eq!(text.chars_before(2).collect::<String>(), "ba");
    }
}
