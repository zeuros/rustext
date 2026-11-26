//! What screen readers get (AccessKit), like egui's own `TextEdit` gives them: the visible
//! lines as text runs of the editor's node, and the selection in them. The runs come from a
//! galley of the visible document text alone (no inlay hints, no ghost text), handed to
//! egui's `update_accesskit_for_text_widget`. Built only when AccessKit is on.

use egui::emath::TSTransform;
use egui::text::CCursor;
use egui::text_selection::CCursorRange;
use egui::text_selection::accesskit_text::update_accesskit_for_text_widget;
use egui::{Id, Role, Ui, vec2};

use super::{CodeEditor, Metrics};

/// A visible row of the editor's text: `line`'s chars `start_column..end_column`.
pub(super) struct VisibleSegment {
    pub line: usize,
    pub start_column: usize,
    pub end_column: usize,
}

impl CodeEditor<'_> {
    pub(super) fn update_accesskit_text(
        &self,
        ui: &Ui,
        editor_id: Id,
        metrics: &Metrics,
        first_row_top: f32,
        segments: &[VisibleSegment],
    ) {
        let is_accesskit_on =
            ui.ctx().accesskit_node_builder(editor_id, |node| node.set_role(Role::MultilineTextInput));
        if is_accesskit_on.is_none() || segments.is_empty() {
            return;
        }
        let text = self.state.text();

        // The visible text, a line break after each segment that ends its line; where each
        // segment starts in it, in chars.
        let mut visible_text = String::new();
        let mut segment_starts = Vec::with_capacity(segments.len());
        let mut char_count = 0;
        for segment in segments {
            let content = text.line_content(segment.line);
            let piece: String =
                content.chars().skip(segment.start_column).take(segment.end_column - segment.start_column).collect();
            segment_starts.push(char_count);
            char_count += piece.chars().count();
            visible_text.push_str(&piece);
            let ends_line = segment.end_column >= content.chars().count();
            if ends_line && segment.line < text.last_line() {
                visible_text.push('\n');
                char_count += 1;
            }
        }

        let visible_index = |offset: usize| {
            let position = text.position(offset);
            segments.iter().zip(&segment_starts).find_map(|(segment, &segment_start)| {
                let holds = segment.line == position.line
                    && (segment.start_column..=segment.end_column).contains(&position.column);
                holds.then(|| CCursor::new(segment_start + position.column - segment.start_column))
            })
        };
        let primary = self.state.selections().primary();
        let cursor_range = visible_index(primary.anchor)
            .zip(visible_index(primary.head))
            .map(|(anchor, head)| CCursorRange { primary: head, secondary: anchor, h_pos: None });

        let galley = ui.painter().layout_no_wrap(visible_text, metrics.font_id(), self.theme.foreground);
        let galley_origin = vec2(self.text_left(metrics), first_row_top);
        update_accesskit_for_text_widget(
            ui.ctx(),
            editor_id,
            cursor_range,
            TSTransform::from_translation(galley_origin),
            &galley,
        );
    }
}
