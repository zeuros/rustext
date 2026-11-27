//! Suspicious characters boxed on the visible lines, explained on hover (D32).

use std::ops::RangeInclusive;

use rustext_core::decoration::{Decoration, DecorationKind};
use rustext_core::unicode::{SuspiciousChar, suspicious_chars};

use super::CodeEditor;
use super::layout::to_rgba;

impl CodeEditor<'_> {
    /// The suspicious characters of `line`, none on a line too long to be painted in full.
    fn suspicious_chars_of(&self, line: usize) -> Vec<SuspiciousChar> {
        let content = self.state.text().line_content(line);
        if content.is_ascii() || content.chars().count() > self.config.max_rendered_line_chars {
            return Vec::new();
        }
        suspicious_chars(&content, &self.config.unicode_highlight)
    }

    /// A box around each suspicious character of `lines`.
    pub(super) fn unicode_decorations(&self, lines: RangeInclusive<usize>) -> Vec<Decoration> {
        let text = self.state.text();
        let border_color = to_rgba(self.theme.unicode_highlight_border);
        lines
            .filter(|&line| line < text.line_count())
            .flat_map(|line| {
                let line_start = text.line_start(line);
                self.suspicious_chars_of(line).into_iter().map(move |found| {
                    let offset = line_start + found.column;
                    Decoration::new(offset, offset + 1, DecorationKind::Border(border_color))
                })
            })
            .collect()
    }

    /// What the suspicious character at `offset` is, for its hover.
    pub(super) fn suspicious_char_explanation(&self, offset: usize) -> Option<String> {
        let text = self.state.text();
        let position = text.position(offset);
        let found =
            self.suspicious_chars_of(position.line).into_iter().find(|found| found.column == position.column)?;
        Some(found.explanation())
    }
}
