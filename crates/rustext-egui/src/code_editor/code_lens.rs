//! Code lenses: labels in a row above their line, aligned with its indentation, separated by
//! ` | `; a click on one is reported, the application decides what it does (D25).

use egui::{Align2, FontId, Painter, pos2};

use super::{CodeEditor, Metrics};

/// Code lens text, smaller than the code's.
const CODE_LENS_FONT_SCALE: f32 = 0.85;
const SEPARATOR: &str = " | ";

impl CodeEditor<'_> {
    /// The lines that have code lenses, sorted, each once.
    pub(super) fn code_lens_lines(&self) -> Vec<usize> {
        let line_count = self.state.text().line_count();
        let mut lines: Vec<usize> =
            self.code_lenses.iter().map(|lens| lens.line).filter(|&line| line < line_count).collect();
        lines.sort_unstable();
        lines.dedup();
        lines
    }

    fn code_lens_font(metrics: &Metrics) -> FontId {
        FontId::monospace(metrics.font_size * CODE_LENS_FONT_SCALE)
    }

    /// Each lens of `line` as (its index in `code_lenses`, left x, right x).
    fn code_lens_spans(&self, metrics: &Metrics, line: usize) -> Vec<(usize, f32, f32)> {
        let content = self.state.text().line_content(line);
        let indentation = &self.state.config.indentation;
        let indent_columns = content.chars().take_while(|character| character.is_whitespace()).count();
        let char_width = metrics.char_width * CODE_LENS_FONT_SCALE;
        let mut left =
            self.text_left(metrics) + indentation.visual_column(&content, indent_columns) as f32 * metrics.char_width;
        let mut spans = Vec::new();
        for (index, lens) in self.code_lenses.iter().enumerate().filter(|(_, lens)| lens.line == line) {
            let right = left + lens.label.chars().count() as f32 * char_width;
            spans.push((index, left, right));
            left = right + SEPARATOR.chars().count() as f32 * char_width;
        }
        spans
    }

    /// The lens of `line` under x, if any.
    pub(super) fn code_lens_at(&self, metrics: &Metrics, line: usize, x: f32) -> Option<usize> {
        self.code_lens_spans(metrics, line)
            .into_iter()
            .find(|&(_, left, right)| (left..right).contains(&x))
            .map(|(index, ..)| index)
    }

    pub(super) fn paint_code_lens_row(
        &self,
        painter: &Painter,
        metrics: &Metrics,
        line: usize,
        top: f32,
        hovered: Option<usize>,
    ) {
        let font_id = Self::code_lens_font(metrics);
        let center_y = top + metrics.row_height / 2.0;
        let spans = self.code_lens_spans(metrics, line);
        for (position, &(index, left, right)) in spans.iter().enumerate() {
            let color = if hovered == Some(index) { self.theme.foreground } else { self.theme.line_number };
            painter.text(
                pos2(left, center_y),
                Align2::LEFT_CENTER,
                &self.code_lenses[index].label,
                font_id.clone(),
                color,
            );
            if position + 1 < spans.len() {
                painter.text(
                    pos2(right, center_y),
                    Align2::LEFT_CENTER,
                    SEPARATOR,
                    font_id.clone(),
                    self.theme.line_number,
                );
            }
        }
    }
}
