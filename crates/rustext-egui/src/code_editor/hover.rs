//! What the hover says: the messages of the decorations under the pointer, links, suspicious
//! characters.

use rustext_core::decoration::{Decoration, DecorationKind};

use super::{CodeEditor, MouseTarget};

impl CodeEditor<'_> {
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
