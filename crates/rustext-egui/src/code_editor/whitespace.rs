//! Whitespace rendering: `·` on spaces and `→` on tabs (Monaco's `renderWhitespace`).

use egui::{Painter, Stroke, pos2};
use rustext_core::{Selections, Text};

use super::layout::LineLayout;
use super::{CodeEditor, Metrics, WhitespaceRendering};

impl CodeEditor<'_> {
    /// `·` on spaces and `→` on tabs, where the whitespace rendering mode wants them.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint_whitespace(
        &self,
        painter: &Painter,
        metrics: &Metrics,
        text: &Text,
        line: usize,
        layout: &LineLayout,
        top: f32,
        selections: &Selections,
    ) {
        let mode = self.config.render_whitespace;
        if mode == WhitespaceRendering::None {
            return;
        }
        let chars: Vec<char> = text.line_content(line).chars().collect();
        let line_start = text.line_start(line);
        let trailing_start = chars.iter().rposition(|character| !character.is_whitespace()).map_or(0, |last| last + 1);
        let is_whitespace =
            |column: usize| chars.get(column).is_some_and(|character| *character == ' ' || *character == '\t');
        let text_left = self.text_left(metrics);
        let center_y = top + metrics.row_height / 2.0;
        let segment_columns = layout.first_column..layout.end_column();
        for (column, &character) in chars.iter().enumerate().take(self.config.max_rendered_line_chars) {
            if !segment_columns.contains(&column) || (character != ' ' && character != '\t') {
                continue;
            }
            let is_shown = match mode {
                WhitespaceRendering::None => false,
                WhitespaceRendering::All => true,
                WhitespaceRendering::Trailing => column >= trailing_start,
                WhitespaceRendering::Boundary => {
                    character == '\t'
                        || column == 0
                        || column >= trailing_start
                        || is_whitespace(column + 1)
                        || is_whitespace(column.wrapping_sub(1))
                }
                WhitespaceRendering::Selection => {
                    let offset = line_start + column;
                    selections.iter().any(|selection| selection.start() <= offset && offset < selection.end())
                }
            };
            if !is_shown {
                continue;
            }
            let left = text_left + layout.x_of_column(column);
            let right = text_left + layout.x_of_column(column + 1);
            if character == '\t' {
                let start = pos2(left + metrics.char_width * 0.2, center_y);
                let end = pos2((right - metrics.char_width * 0.2).max(start.x + 2.0), center_y);
                painter.arrow(start, end - start, Stroke::new(1.0, self.theme.whitespace));
            } else {
                painter.circle_filled(pos2((left + right) / 2.0, center_y), 1.2, self.theme.whitespace);
            }
        }
    }
}
