//! What the hover says: the messages of the decorations under the pointer, links, suspicious
//! characters.

use rustext_core::decoration::{Decoration, DecorationKind};

use egui::{Pos2, Ui};

use super::{CodeEditor, Metrics, MouseTarget};

impl CodeEditor<'_> {
    /// Char offset of the char under `pos` (not the nearest boundary), if `pos` is over text.
    pub(super) fn char_under(&mut self, ui: &Ui, metrics: &Metrics, pos: Pos2) -> Option<usize> {
        let row = self.row_at_y(metrics, pos.y);
        let (line, start_column, end_column) = self.row_segment(row)?;
        let layout = self.layout_segment(ui, metrics, line, start_column, end_column);
        let x = pos.x - self.text_left(metrics);
        let nearest_column = layout.column_at_x(x);
        let column = if nearest_column > 0 && x < layout.x_of_column(nearest_column) {
            nearest_column - 1
        } else {
            nearest_column
        };
        (column < self.state.text().line_len(line)).then(|| self.state.text().line_start(line) + column)
    }

    /// Messages of the decorations under the pointer: on the text, or in the gutter for glyphs.
    pub(super) fn hover_messages(&self, target: MouseTarget, hovered_char: Option<usize>) -> Vec<String> {
        let text = self.state.text();
        let decorations = self.all_decorations().filter(|decoration| decoration.hover_message.is_some());
        let hovered: Vec<&Decoration> = match target {
            MouseTarget::Text { .. } => match hovered_char {
                Some(offset) => decorations
                    .filter(|decoration| {
                        decoration.start <= offset && offset < decoration.end.max(decoration.start + 1)
                    })
                    .collect(),
                None => Vec::new(),
            },
            MouseTarget::GlyphMargin { line } | MouseTarget::LineNumber { line } => decorations
                .filter(|decoration| {
                    let is_gutter_kind =
                        matches!(decoration.kind, DecorationKind::Glyph(_) | DecorationKind::LineNumberColor(_));
                    is_gutter_kind && decoration.first_line(text) == line
                })
                .collect(),
            _ => Vec::new(),
        };
        let link_message = match (target, hovered_char) {
            (MouseTarget::Text { .. }, Some(offset)) if self.link_at(offset).is_some() => {
                Some("Follow link (Ctrl+click)".to_string())
            }
            _ => None,
        };
        let unicode_message = match (target, hovered_char) {
            (MouseTarget::Text { .. }, Some(offset)) => self.suspicious_char_explanation(offset),
            _ => None,
        };
        hovered
            .into_iter()
            .filter_map(|decoration| decoration.hover_message.clone())
            .chain(link_message)
            .chain(unicode_message)
            .collect()
    }
}
