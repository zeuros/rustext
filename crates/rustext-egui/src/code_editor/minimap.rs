//! The minimap (Monaco's `minimap`): rows at two points each, one point per char, in their
//! token colors, marks behind them (find matches, selections, decorations with a minimap color,
//! such as diagnostics: DECISIONS D35), and a slider over the rows the viewport shows.

use egui::{Color32, CornerRadius, Painter, Rect, pos2, vec2};
use rustext_core::Text;

use super::layout::to_color;
use super::{CodeEditor, Metrics};

/// Height of a minimap row, in points.
pub(super) const MINIMAP_ROW_HEIGHT: f32 = 2.0;

/// Columns the minimap shows; longer rows are cut.
const MAX_COLUMNS: usize = 120;

/// Space left of the minimap's first column, in points.
const LEFT_PADDING: f32 = 3.0;

/// A span colored behind the minimap's text, in char offsets.
struct MinimapMark {
    start: usize,
    end: usize,
    color: Color32,
}

/// Where the minimap's columns are drawn.
#[derive(Clone, Copy)]
struct MinimapColumns {
    left: f32,
    column_width: f32,
}

impl MinimapColumns {
    /// Left edge of `column` (of the row's segment).
    fn x_of(self, column: usize) -> f32 {
        self.left + LEFT_PADDING + column as f32 * self.column_width
    }
}

/// Where a row of the minimap is drawn and what it shows.
struct MinimapRow {
    top: f32,
    line: usize,
    start_column: usize,
    end_column: usize,
}

impl CodeEditor<'_> {
    pub(super) fn paint_minimap(&mut self, painter: &Painter, metrics: &Metrics, text: &Text) {
        let rect = metrics.minimap_rect;
        if rect.width() <= 0.0 {
            return;
        }
        let minimap_painter = painter.with_clip_rect(rect);
        minimap_painter.rect_filled(rect, CornerRadius::ZERO, self.theme.background);
        let columns = MinimapColumns {
            left: rect.left(),
            column_width: (rect.width() - 2.0 * LEFT_PADDING) / MAX_COLUMNS as f32,
        };

        let first_row = self.minimap_first_row(metrics);
        let row_count = (rect.height() / MINIMAP_ROW_HEIGHT) as usize + 1;
        let rows: Vec<MinimapRow> = (first_row..(first_row + row_count).min(self.row_count()))
            .filter_map(|row| {
                let (line, start_column, end_column) = self.row_segment(row)?;
                let top = rect.top() + (row - first_row) as f32 * MINIMAP_ROW_HEIGHT;
                Some(MinimapRow { top, line, start_column, end_column })
            })
            .collect();
        let marks = self.minimap_marks(text, &rows);
        for row in &rows {
            for mark in marks.iter() {
                paint_mark(&minimap_painter, text, row, mark, columns);
            }
            self.paint_minimap_row(&minimap_painter, text, row, columns);
        }

        let first_visible_row = self.row_at_y(metrics, metrics.text_rect.top());
        let visible_rows = (metrics.text_rect.height() / metrics.row_height).ceil();
        let slider_top = rect.top() + first_visible_row.saturating_sub(first_row) as f32 * MINIMAP_ROW_HEIGHT;
        let slider =
            Rect::from_min_size(pos2(rect.left(), slider_top), vec2(rect.width(), visible_rows * MINIMAP_ROW_HEIGHT));
        minimap_painter.rect_filled(slider, CornerRadius::ZERO, self.theme.scrollbar_thumb.gamma_multiply(0.6));
    }

    /// The marks on the lines of `rows`: find matches, non-empty selections, and decorations
    /// with a minimap color.
    fn minimap_marks(&self, text: &Text, rows: &[MinimapRow]) -> Vec<MinimapMark> {
        let (Some(first), Some(last)) = (rows.first(), rows.last()) else { return Vec::new() };
        let (shown_start, shown_end) = (text.line_start(first.line), text.line_content_end(last.line));
        let find_color = self.theme.find_match_background.gamma_multiply(1.5);
        let selection_color = self.theme.selection_background;

        let found = self.view.find.matches().iter().map(|found| MinimapMark {
            start: found.start,
            end: found.end,
            color: find_color,
        });
        let selected = self
            .state
            .selections()
            .iter()
            .filter(|selection| !selection.is_empty())
            .map(|selection| MinimapMark { start: selection.start(), end: selection.end(), color: selection_color });
        let decorated = self.all_decorations().filter_map(|decoration| {
            let color = to_color(decoration.minimap?);
            Some(MinimapMark { start: decoration.start, end: decoration.end, color })
        });
        found
            .chain(selected)
            .chain(decorated)
            .filter(|mark| mark.start <= shown_end && mark.end >= shown_start)
            .collect()
    }

    /// The chars of one row, as runs of their token color.
    fn paint_minimap_row(&mut self, painter: &Painter, text: &Text, row: &MinimapRow, columns: MinimapColumns) {
        let spans = self.styler.as_deref_mut().map(|styler| styler.style_line(text, row.line)).unwrap_or_default();
        let segment_len = row.end_column.saturating_sub(row.start_column).min(MAX_COLUMNS);
        let content: Vec<char> = text.line_content(row.line).chars().skip(row.start_column).take(segment_len).collect();
        let mut column = 0;
        while column < content.len() {
            if content[column].is_whitespace() {
                column += 1;
                continue;
            }
            let absolute_column = row.start_column + column;
            let span =
                spans.iter().find(|span| span.start_column <= absolute_column && absolute_column < span.end_column);
            let color = span.and_then(|span| span.style.foreground).map(to_color).unwrap_or(self.theme.foreground);
            let run_end = (column..content.len())
                .find(|&end| {
                    content[end].is_whitespace() || span.is_some_and(|span| row.start_column + end >= span.end_column)
                })
                .unwrap_or(content.len());
            let run_left = columns.x_of(column);
            let size = vec2((run_end - column) as f32 * columns.column_width, MINIMAP_ROW_HEIGHT - 0.5);
            painter.rect_filled(
                Rect::from_min_size(pos2(run_left, row.top), size),
                CornerRadius::ZERO,
                color.gamma_multiply(0.7),
            );
            column = run_end.max(column + 1);
        }
    }

    /// The first row the minimap shows: it scrolls proportionally when the text is taller.
    pub(super) fn minimap_first_row(&self, metrics: &Metrics) -> usize {
        let minimap_rows = (metrics.minimap_rect.height() / MINIMAP_ROW_HEIGHT) as usize;
        let hidden_rows = self.row_count().saturating_sub(minimap_rows);
        let fraction =
            if self.max_scroll_y(metrics) > 0.0 { self.view.scroll_offset.y / self.max_scroll_y(metrics) } else { 0.0 };
        (fraction * hidden_rows as f32).round() as usize
    }

    /// Scrolls so the row under `y` in the minimap is in the middle of the viewport.
    pub(super) fn scroll_to_minimap_y(&mut self, metrics: &Metrics, y: f32) {
        let row =
            self.minimap_first_row(metrics) as f32 + ((y - metrics.minimap_rect.top()) / MINIMAP_ROW_HEIGHT).max(0.0);
        self.view.scroll_offset.y = row * metrics.row_height - metrics.text_rect.height() / 2.0;
    }
}

/// The part of `mark` on `row`, at least a column wide so a short mark still shows.
fn paint_mark(painter: &Painter, text: &Text, row: &MinimapRow, mark: &MinimapMark, columns: MinimapColumns) {
    let line_start = text.line_start(row.line);
    let line_end = text.line_content_end(row.line);
    if mark.start > line_end || mark.end < line_start {
        return;
    }
    let start_column = (mark.start.max(line_start) - line_start).max(row.start_column);
    let end_column = (mark.end.min(line_end) - line_start).min(row.end_column);
    if start_column > end_column || start_column >= row.start_column + MAX_COLUMNS {
        return;
    }
    let columns_shown = (end_column - start_column).max(1).min(MAX_COLUMNS + row.start_column - start_column);
    let mark_left = columns.x_of(start_column - row.start_column);
    let size = vec2(columns_shown as f32 * columns.column_width, MINIMAP_ROW_HEIGHT);
    painter.rect_filled(Rect::from_min_size(pos2(mark_left, row.top), size), CornerRadius::ZERO, mark.color);
}
