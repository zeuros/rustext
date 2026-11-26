//! Painting the visible rows, the gutter, cursors and the scrollbar.

use egui::{Align2, Color32, CornerRadius, Painter, Rect, Stroke, StrokeKind, Ui, pos2, vec2};
use rustext_core::decoration::{Decoration, DecorationKind};
use rustext_core::display::RowContent;
use rustext_core::{Position, Selection, Selections, Text};

use super::layout::{LineLayout, to_color};
use super::structure::folded_header_lines;
use super::{CodeEditor, CurrentLineHighlight, CursorStyle, Drag, LineNumbers, Metrics, WhitespaceRendering};
use crate::theme::EditorTheme;

/// What the gutter needs to know about the cursors and folds.
struct GutterContext {
    cursor_lines: Vec<usize>,
    primary_head: Position,
    highlights_current_line: bool,
    fold_header_lines: Vec<usize>,
    folded_header_lines: Vec<usize>,
    is_hovered: bool,
}

/// A visible row with its laid-out text, when it shows a line.
struct PaintedRow {
    row: usize,
    content: RowContent,
    layout: Option<LineLayout>,
}

impl CodeEditor<'_> {
    pub(super) fn paint(&mut self, ui: &mut Ui, metrics: &Metrics, has_focus: bool) {
        let painter = ui.painter_at(metrics.full_rect());
        let theme = self.theme.clone();
        painter.rect_filled(metrics.full_rect(), CornerRadius::ZERO, theme.background);

        let text = self.state.text().clone();
        let selections = self.state.selections().clone();
        let primary_head = text.position(selections.primary().head);
        let cursor_lines: Vec<usize> = selections.iter().map(|selection| text.line_of_offset(selection.head)).collect();
        let highlights_current_line = selections.iter().all(Selection::is_empty);
        let text_painter = painter.with_clip_rect(metrics.text_rect);
        let visible_rows = self.visible_rows(metrics);
        let visible_start = text.line_start(self.nearest_line(visible_rows.start));
        let visible_end = text.line_content_end(
            self.nearest_line(visible_rows.end.saturating_sub(1)).max(self.nearest_line(visible_rows.start)),
        );
        let find_decorations = self.find_decorations(visible_start, visible_end.max(visible_start));
        let mut structure_decorations = self.occurrence_decorations(visible_start, visible_end.max(visible_start));
        structure_decorations.extend(self.bracket_decorations());
        let first_visible_line = self.nearest_line(visible_rows.start);
        let last_visible_line = self.nearest_line(visible_rows.end.saturating_sub(1)).max(first_visible_line);
        let (indent_levels, active_guide) = if self.config.indent_guides {
            self.indent_guides(first_visible_line..last_visible_line + 1, primary_head.line)
        } else {
            (Vec::new(), None)
        };
        let folded_headers = folded_header_lines(&self.view.folds);

        let mut rows = Vec::new();
        for row in visible_rows.clone() {
            let content = self.row_content(row);
            let layout = match content {
                RowContent::Line(_) | RowContent::Wrapped { .. } => {
                    self.row_segment(row).map(|(line, start_column, end_column)| {
                        self.layout_segment(ui, metrics, line, start_column, end_column)
                    })
                }
                RowContent::Foreign(line) => self.layout_foreign_line(ui, metrics, line),
                RowContent::Padding | RowContent::Collapsed { .. } => None,
            };
            if let Some(layout) = &layout {
                self.view.widest_line_px = self.view.widest_line_px.max(layout.width());
            }
            rows.push(PaintedRow { row, content, layout });
        }

        for painted in &rows {
            let top = self.row_top(metrics, painted.row);
            let row_rect = Rect::from_min_size(
                pos2(metrics.text_rect.left(), top),
                vec2(metrics.text_rect.width(), metrics.row_height),
            );
            let paints_current_line =
                matches!(self.config.current_line_highlight, CurrentLineHighlight::Line | CurrentLineHighlight::All);
            let is_cursor_row = matches!(painted.content, RowContent::Line(line) | RowContent::Wrapped { line, .. } if cursor_lines.contains(&line));
            if paints_current_line && highlights_current_line && is_cursor_row {
                text_painter.rect_filled(row_rect, CornerRadius::ZERO, theme.current_line_background);
            }
            if let (Some(background), _) = self.row_style(painted.row) {
                text_painter.rect_filled(row_rect, CornerRadius::ZERO, to_color(background));
            }
            match (painted.content, &painted.layout) {
                (RowContent::Line(line) | RowContent::Wrapped { line, .. }, Some(layout)) => {
                    let is_first_segment = matches!(painted.content, RowContent::Line(_));
                    let is_last_segment = layout.end_column() >= text.line_len(line);
                    self.paint_line_decorations(&text_painter, metrics, &text, line, layout, top, self.decorations);
                    let service_diagnostics = &self.view.service_diagnostics;
                    self.paint_line_decorations(&text_painter, metrics, &text, line, layout, top, service_diagnostics);
                    self.paint_line_decorations(&text_painter, metrics, &text, line, layout, top, &find_decorations);
                    self.paint_line_decorations(
                        &text_painter,
                        metrics,
                        &text,
                        line,
                        layout,
                        top,
                        &structure_decorations,
                    );
                    self.paint_selections(&text_painter, metrics, &text, line, layout, top, &selections, has_focus);
                    let level = line.checked_sub(first_visible_line).and_then(|index| indent_levels.get(index));
                    if let Some(&level) = level.filter(|_| is_first_segment) {
                        self.paint_indent_guides(&text_painter, metrics, line, level, top, active_guide);
                    }
                    self.paint_galley(&text_painter, metrics, layout, top);
                    self.paint_whitespace(&text_painter, metrics, &text, line, layout, top, &selections);
                    if is_last_segment
                        && self.config.show_carriage_returns
                        && text.line_ending_of(line) == Some(rustext_core::LineEnding::CrLf)
                    {
                        self.paint_end_of_line_label(&text_painter, metrics, layout, top, "CR", theme.whitespace);
                    }
                    if is_last_segment && folded_headers.binary_search(&line).is_ok() {
                        self.paint_end_of_line_label(&text_painter, metrics, layout, top, "⋯", theme.foreground);
                    }
                }
                (RowContent::Foreign(line), Some(layout)) => {
                    if let Some(foreign) = &self.foreign {
                        self.paint_line_decorations(
                            &text_painter,
                            metrics,
                            foreign.text,
                            line,
                            layout,
                            top,
                            foreign.decorations,
                        );
                    }
                    self.paint_galley(&text_painter, metrics, layout, top);
                }
                (RowContent::Padding, _) => paint_padding(&text_painter, row_rect, theme.diff_padding_background),
                (RowContent::Collapsed { first_line, last_line }, _) => {
                    let full_row = Rect::from_min_max(
                        pos2(metrics.full_rect().left(), top),
                        pos2(metrics.text_rect.right(), top + metrics.row_height),
                    );
                    painter.rect_filled(full_row, CornerRadius::ZERO, theme.widget_background);
                    let hidden_count = last_line - first_line + 1;
                    let label = format!("⋯ {hidden_count} unchanged line{}", if hidden_count == 1 { "" } else { "s" });
                    let label_pos = pos2(metrics.text_rect.left() + metrics.char_width, top + metrics.row_height / 2.0);
                    painter.text(label_pos, Align2::LEFT_CENTER, label, metrics.font_id(), theme.line_number);
                }
                _ => {}
            }
        }

        let text_left = self.text_left(metrics);
        if let Some(placeholder) = self.config.placeholder.as_ref().filter(|_| text.is_empty()) {
            let top = self.row_top(metrics, 0) + metrics.row_height / 2.0;
            let font_id = metrics.font_id();
            text_painter.text(pos2(text_left, top), Align2::LEFT_CENTER, placeholder, font_id, theme.line_number);
        }
        for column in &self.config.rulers {
            let x = text_left + *column as f32 * metrics.char_width;
            text_painter.vline(x, metrics.text_rect.y_range(), Stroke::new(1.0, theme.ruler));
        }

        let now = ui.input(|input| input.time);
        let blink_on = !self.config.cursor_blinking || ((now - self.view.last_interaction_time) % 1.0) < 0.55;
        // The row showing a position: the one whose segment holds its column (the later one at
        // a wrap point).
        let row_of_position = |position: Position| {
            rows.iter()
                .filter(|painted| matches!(painted.content, RowContent::Line(line) | RowContent::Wrapped { line, .. } if line == position.line))
                .filter_map(|painted| painted.layout.as_ref().map(|layout| (painted.row, layout)))
                .take_while(|(_, layout)| layout.first_column <= position.column)
                .last()
        };
        if has_focus && blink_on {
            for selection in selections.iter() {
                let head = text.position(selection.head);
                if let Some((row, layout)) = row_of_position(head) {
                    self.paint_cursor(&text_painter, metrics, row, layout, head, &theme);
                }
            }
        }
        if has_focus {
            if self.config.cursor_blinking {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
            }
            if let Some((row, layout)) = row_of_position(primary_head) {
                self.paint_ime(ui, &text_painter, metrics, row, layout, primary_head, &theme);
            }
        }
        if self.styler.as_deref().is_some_and(|styler| styler.is_catching_up()) {
            ui.ctx().request_repaint();
        }

        let paints_gutter_line =
            matches!(self.config.current_line_highlight, CurrentLineHighlight::Gutter | CurrentLineHighlight::All);
        let fold_header_lines = if metrics.fold_marker_width > 0.0 {
            let visible_lines = rows.iter().filter_map(|painted| match painted.content {
                RowContent::Line(line) => Some(line),
                _ => None,
            });
            visible_lines.filter(|&line| self.is_fold_header(line)).collect()
        } else {
            Vec::new()
        };
        let is_hovered =
            ui.input(|input| input.pointer.hover_pos()).is_some_and(|pos| metrics.gutter_rect().contains(pos));
        let gutter_context = GutterContext {
            cursor_lines,
            primary_head,
            highlights_current_line: paints_gutter_line && highlights_current_line,
            fold_header_lines,
            folded_header_lines: folded_headers,
            is_hovered,
        };
        self.paint_gutter(&painter, metrics, &text, &rows, &gutter_context);
        self.view.sticky_lines = self.compute_sticky_lines(metrics);
        self.paint_sticky_lines(ui, &painter, metrics, primary_head.line);
        self.paint_minimap(&painter, metrics, &text);
        self.paint_scrollbar(ui, &painter, metrics, &text, &selections);
    }

    /// Rows at two points each, one point per char, in their token colors, and a slider over
    /// the rows the viewport shows.
    fn paint_minimap(&mut self, painter: &Painter, metrics: &Metrics, text: &Text) {
        const MAX_COLUMNS: usize = 120;
        let rect = metrics.minimap_rect;
        if rect.width() <= 0.0 {
            return;
        }
        let theme = self.theme.clone();
        let minimap_painter = painter.with_clip_rect(rect);
        minimap_painter.rect_filled(rect, CornerRadius::ZERO, theme.background);
        let column_width = (rect.width() - 6.0) / MAX_COLUMNS as f32;
        let first_row = self.minimap_first_row(metrics);
        let row_count = (rect.height() / super::MINIMAP_ROW_HEIGHT) as usize + 1;
        for row in first_row..(first_row + row_count).min(self.row_count()) {
            let Some((line, start_column, end_column)) = self.row_segment(row) else { continue };
            let spans = self.styler.as_deref_mut().map(|styler| styler.style_line(text, line)).unwrap_or_default();
            let top = rect.top() + (row - first_row) as f32 * super::MINIMAP_ROW_HEIGHT;
            let segment_len = end_column.saturating_sub(start_column).min(MAX_COLUMNS);
            let content: Vec<char> = text.line_content(line).chars().skip(start_column).take(segment_len).collect();
            let mut column = 0;
            while column < content.len() {
                if content[column].is_whitespace() {
                    column += 1;
                    continue;
                }
                let absolute_column = start_column + column;
                let span =
                    spans.iter().find(|span| span.start_column <= absolute_column && absolute_column < span.end_column);
                let color = span.and_then(|span| span.style.foreground).map(to_color).unwrap_or(theme.foreground);
                let run_end = (column..content.len())
                    .find(|&end| {
                        content[end].is_whitespace() || span.is_some_and(|span| start_column + end >= span.end_column)
                    })
                    .unwrap_or(content.len());
                let left = rect.left() + 3.0 + column as f32 * column_width;
                let size = vec2((run_end - column) as f32 * column_width, super::MINIMAP_ROW_HEIGHT - 0.5);
                minimap_painter.rect_filled(
                    Rect::from_min_size(pos2(left, top), size),
                    CornerRadius::ZERO,
                    color.gamma_multiply(0.7),
                );
                column = run_end.max(column + 1);
            }
        }
        let first_visible_row = self.row_at_y(metrics, metrics.text_rect.top());
        let visible_rows = (metrics.text_rect.height() / metrics.row_height).ceil();
        let slider_top = rect.top() + first_visible_row.saturating_sub(first_row) as f32 * super::MINIMAP_ROW_HEIGHT;
        let slider = Rect::from_min_size(
            pos2(rect.left(), slider_top),
            vec2(rect.width(), visible_rows * super::MINIMAP_ROW_HEIGHT),
        );
        minimap_painter.rect_filled(slider, CornerRadius::ZERO, theme.scrollbar_thumb.gamma_multiply(0.6));
    }

    /// The pinned headers over the top rows, gutter included, with a line under them.
    fn paint_sticky_lines(&mut self, ui: &Ui, painter: &Painter, metrics: &Metrics, cursor_line: usize) {
        if self.view.sticky_lines.is_empty() {
            return;
        }
        let theme = self.theme.clone();
        let full_left = metrics.gutter_rect().left();
        let text_painter = painter.with_clip_rect(metrics.text_rect);
        let numbers_right = full_left + metrics.glyph_margin_width + metrics.line_numbers_width;
        let lines = self.view.sticky_lines.clone();
        for (slot, line) in lines.iter().enumerate() {
            let top = metrics.text_rect.top() + slot as f32 * metrics.row_height;
            let row_rect =
                Rect::from_min_max(pos2(full_left, top), pos2(metrics.text_rect.right(), top + metrics.row_height));
            painter.rect_filled(row_rect, CornerRadius::ZERO, theme.background);
            let layout = self.layout_line(ui, metrics, *line);
            let text_top = top + (metrics.row_height - layout.galley.size().y) / 2.0;
            text_painter.galley(pos2(self.text_left(metrics), text_top), layout.galley.clone(), theme.foreground);
            if self.config.line_numbers != LineNumbers::Off {
                let color = if *line == cursor_line { theme.active_line_number } else { theme.line_number };
                let number_pos = pos2(numbers_right, top + metrics.row_height / 2.0);
                painter.text(number_pos, Align2::RIGHT_CENTER, (line + 1).to_string(), metrics.font_id(), color);
            }
        }
        let bottom = metrics.text_rect.top() + lines.len() as f32 * metrics.row_height;
        painter.hline(full_left..=metrics.text_rect.right(), bottom, Stroke::new(1.0, theme.widget_border));
    }

    fn paint_galley(&self, painter: &Painter, metrics: &Metrics, layout: &LineLayout, top: f32) {
        let text_top = top + (metrics.row_height - layout.galley.size().y) / 2.0;
        painter.galley(
            pos2(self.text_left(metrics) + layout.indent_px, text_top),
            layout.galley.clone(),
            self.theme.foreground,
        );
    }

    /// Vertical lines at each indentation level of the line; the guide of the cursor's block
    /// (`active_guide`: level, first line, last line) in a brighter color.
    fn paint_indent_guides(
        &self,
        painter: &Painter,
        metrics: &Metrics,
        line: usize,
        level: usize,
        top: f32,
        active_guide: Option<(usize, usize, usize)>,
    ) {
        let indent_width = self.state.config.indentation.tab_size as f32 * metrics.char_width;
        let text_left = self.text_left(metrics);
        for guide in 0..level {
            let x = (text_left + guide as f32 * indent_width).round() + 0.5;
            let is_active = active_guide.is_some_and(|(active_level, first_line, last_line)| {
                active_level == guide && (first_line..=last_line).contains(&line)
            });
            let color = if is_active { self.theme.active_indent_guide } else { self.theme.indent_guide };
            painter.vline(x, top..=top + metrics.row_height, Stroke::new(1.0, color));
        }
    }

    /// `·` on spaces and `→` on tabs, where the whitespace rendering mode wants them.
    #[allow(clippy::too_many_arguments)]
    fn paint_whitespace(
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

    /// A small label after the line's text: `CR` for a `\r\n` line, `⋯` for a folded line.
    fn paint_end_of_line_label(
        &self,
        painter: &Painter,
        metrics: &Metrics,
        layout: &LineLayout,
        top: f32,
        label: &str,
        color: Color32,
    ) {
        let font_id = egui::FontId::monospace(metrics.font_size * if label == "⋯" { 1.0 } else { 0.7 });
        let galley = painter.layout_no_wrap(label.to_string(), font_id, color);
        let left = self.text_left(metrics) + layout.width() + metrics.char_width * 0.5;
        let padding = vec2(3.0, 1.0);
        let rect = Rect::from_min_size(
            pos2(left, top + (metrics.row_height - galley.size().y) / 2.0 - padding.y),
            galley.size() + padding * 2.0,
        );
        painter.rect_filled(rect, CornerRadius::same(3), self.theme.fold_placeholder_background);
        painter.galley(rect.min + padding, galley, color);
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_selections(
        &self,
        painter: &Painter,
        metrics: &Metrics,
        text: &Text,
        line: usize,
        layout: &LineLayout,
        top: f32,
        selections: &Selections,
        has_focus: bool,
    ) {
        let text_left = self.text_left(metrics);
        let line_start = text.line_start(line);
        let line_end = text.line_content_end(line);
        let color = if has_focus { self.theme.selection_background } else { self.theme.inactive_selection_background };
        for selection in selections.iter().filter(|selection| !selection.is_empty()) {
            if selection.end() < line_start || selection.start() > line_end {
                continue;
            }
            let (start, end) = (selection.start().max(line_start), selection.end().min(line_end));
            let continues_past_line = selection.end() > line_end && layout.end_column() >= line_end - line_start;
            if start == end && !continues_past_line {
                continue;
            }
            let left = text_left + layout.x_of_column(start - line_start);
            let past_line_width = if continues_past_line { metrics.char_width * 0.6 } else { 0.0 };
            let right = text_left + layout.x_of_column(end - line_start) + past_line_width;
            painter.rect_filled(
                Rect::from_min_max(pos2(left, top), pos2(right, top + metrics.row_height)),
                CornerRadius::same(2),
                color,
            );
        }
    }

    /// Whole-line and inline decorations of `line` of `text` (own or foreign).
    #[allow(clippy::too_many_arguments)]
    fn paint_line_decorations(
        &self,
        painter: &Painter,
        metrics: &Metrics,
        text: &Text,
        line: usize,
        layout: &LineLayout,
        top: f32,
        decorations: &[Decoration],
    ) {
        let line_start = text.line_start(line);
        let line_end = text.line_content_end(line);
        let text_left = self.text_left(metrics);
        let bottom = top + metrics.row_height;
        for decoration in
            decorations.iter().filter(|decoration| decoration.end >= line_start && decoration.start <= line_end)
        {
            let (start, end) = (decoration.start.max(line_start), decoration.end.min(line_end));
            let left = text_left + layout.x_of_column(start - line_start);
            let right = text_left + layout.x_of_column(end - line_start);
            match &decoration.kind {
                DecorationKind::WholeLineBackground(color) => {
                    let row_rect = Rect::from_min_max(
                        pos2(metrics.text_rect.left(), top),
                        pos2(metrics.text_rect.right(), bottom),
                    );
                    painter.rect_filled(row_rect, CornerRadius::ZERO, to_color(*color));
                }
                DecorationKind::Background(color) => {
                    painter.rect_filled(
                        Rect::from_min_max(pos2(left, top), pos2(right, bottom)),
                        CornerRadius::ZERO,
                        to_color(*color),
                    );
                }
                DecorationKind::Border(color) => {
                    let rect = Rect::from_min_max(pos2(left, top), pos2(right.max(left + 2.0), bottom));
                    painter.rect_stroke(
                        rect,
                        CornerRadius::same(2),
                        Stroke::new(1.0, to_color(*color)),
                        StrokeKind::Inside,
                    );
                }
                DecorationKind::Squiggle(color) => {
                    paint_squiggle(painter, left, right.max(left + metrics.char_width), bottom - 2.0, to_color(*color));
                }
                DecorationKind::Underline(color) => {
                    painter.hline(left..=right, bottom - 1.5, Stroke::new(1.0, to_color(*color)));
                }
                DecorationKind::Glyph(_)
                | DecorationKind::LineNumberColor(_)
                | DecorationKind::GutterBar(_)
                | DecorationKind::InjectedText { .. } => {}
            }
        }
    }

    fn paint_cursor(
        &self,
        painter: &Painter,
        metrics: &Metrics,
        row: usize,
        layout: &LineLayout,
        head: Position,
        theme: &EditorTheme,
    ) {
        let top = self.row_top(metrics, row);
        let x = self.text_left(metrics) + layout.x_of_column(head.column);
        let next_x = self.text_left(metrics) + layout.x_of_column(head.column + 1);
        let char_width = if next_x > x { next_x - x } else { metrics.char_width };
        let rect = match self.config.cursor_style {
            CursorStyle::Line => Rect::from_min_size(pos2(x - 1.0, top), vec2(2.0, metrics.row_height)),
            CursorStyle::Block => Rect::from_min_size(pos2(x, top), vec2(char_width, metrics.row_height)),
            CursorStyle::Underline => {
                Rect::from_min_size(pos2(x, top + metrics.row_height - 2.0), vec2(char_width, 2.0))
            }
        };
        let color = if self.config.cursor_style == CursorStyle::Block {
            theme.cursor.gamma_multiply(0.6)
        } else {
            theme.cursor
        };
        painter.rect_filled(rect, CornerRadius::ZERO, color);
    }

    /// Shows the IME composition text at the primary cursor and tells the platform where it is.
    #[allow(clippy::too_many_arguments)]
    fn paint_ime(
        &self,
        ui: &Ui,
        painter: &Painter,
        metrics: &Metrics,
        row: usize,
        layout: &LineLayout,
        primary_head: Position,
        theme: &EditorTheme,
    ) {
        let top = self.row_top(metrics, row);
        let x = self.text_left(metrics) + layout.x_of_column(primary_head.column);
        let cursor_rect = Rect::from_min_size(pos2(x, top), vec2(2.0, metrics.row_height));
        if let Some(preedit) = &self.view.ime_preedit {
            let galley = painter.layout_no_wrap(preedit.clone(), metrics.font_id(), theme.foreground);
            let preedit_rect = Rect::from_min_size(pos2(x, top), vec2(galley.size().x, metrics.row_height));
            painter.rect_filled(preedit_rect, CornerRadius::ZERO, theme.background);
            painter.galley(pos2(x, top + (metrics.row_height - galley.size().y) / 2.0), galley, theme.foreground);
            painter.hline(preedit_rect.x_range(), preedit_rect.bottom() - 1.0, Stroke::new(1.0, theme.foreground));
        }
        let to_global = ui.ctx().layer_transform_to_global(ui.layer_id()).unwrap_or_default();
        ui.ctx().output_mut(|output| {
            output.ime = Some(egui::output::IMEOutput {
                purpose: egui::IMEPurpose::Normal,
                rect: to_global * metrics.text_rect,
                cursor_rect: to_global * cursor_rect,
                should_interrupt_composition: false,
            });
        });
    }

    fn paint_gutter(
        &self,
        painter: &Painter,
        metrics: &Metrics,
        text: &Text,
        rows: &[PaintedRow],
        context: &GutterContext,
    ) {
        let (cursor_lines, primary_head) = (&context.cursor_lines, context.primary_head);
        let theme = self.theme;
        let gutter_rect = metrics.gutter_rect();
        let gutter_painter = painter.with_clip_rect(gutter_rect);
        gutter_painter.rect_filled(gutter_rect, CornerRadius::ZERO, theme.gutter_background);
        let numbers_right = gutter_rect.left() + metrics.glyph_margin_width + metrics.line_numbers_width;
        let marker_center_x = numbers_right + metrics.marker_width / 2.0;
        let font_id = metrics.font_id();

        for painted in rows {
            let top = self.row_top(metrics, painted.row);
            let center_y = top + metrics.row_height / 2.0;
            let row_rect =
                Rect::from_min_size(pos2(gutter_rect.left(), top), vec2(gutter_rect.width(), metrics.row_height));
            let (background, marker) = self.row_style(painted.row);
            if let Some(background) = background {
                gutter_painter.rect_filled(row_rect, CornerRadius::ZERO, to_color(background));
            }
            if let Some(marker) = marker.filter(|_| self.markers) {
                gutter_painter.text(
                    pos2(marker_center_x, center_y),
                    Align2::CENTER_CENTER,
                    marker,
                    font_id.clone(),
                    theme.foreground,
                );
            }

            let line = match painted.content {
                RowContent::Line(line) => line,
                RowContent::Foreign(foreign_line) if self.config.line_numbers != LineNumbers::Off => {
                    let number_pos = pos2(numbers_right, center_y);
                    gutter_painter.text(
                        number_pos,
                        Align2::RIGHT_CENTER,
                        (foreign_line + 1).to_string(),
                        font_id.clone(),
                        theme.line_number,
                    );
                    continue;
                }
                RowContent::Collapsed { .. } => {
                    gutter_painter.rect_filled(row_rect, CornerRadius::ZERO, theme.widget_background);
                    continue;
                }
                RowContent::Wrapped { .. } => continue,
                RowContent::Padding | RowContent::Foreign(_) => continue,
            };
            let is_cursor_line = cursor_lines.contains(&line);
            if context.highlights_current_line && is_cursor_line && background.is_none() {
                gutter_painter.rect_filled(row_rect, CornerRadius::ZERO, theme.current_line_background);
            }
            let is_folded = context.folded_header_lines.binary_search(&line).is_ok();
            if context.fold_header_lines.contains(&line) && (is_folded || context.is_hovered) {
                let marker_x = metrics.text_rect.left() - metrics.char_width - metrics.fold_marker_width / 2.0;
                let symbol = if is_folded { "▸" } else { "▾" };
                gutter_painter.text(
                    pos2(marker_x, center_y),
                    Align2::CENTER_CENTER,
                    symbol,
                    font_id.clone(),
                    theme.fold_marker,
                );
            }
            let mut line_number_color = None;
            for decoration in self.all_decorations().filter(|decoration| decoration.covers_line(text, line)) {
                match decoration.kind {
                    DecorationKind::WholeLineBackground(color) => {
                        gutter_painter.rect_filled(row_rect, CornerRadius::ZERO, to_color(color));
                    }
                    DecorationKind::LineNumberColor(color) => line_number_color = Some(to_color(color)),
                    DecorationKind::GutterBar(color) => {
                        let left = metrics.text_rect.left() - metrics.char_width * 0.7;
                        let bar = Rect::from_min_max(pos2(left, top), pos2(left + 3.0, top + metrics.row_height));
                        gutter_painter.rect_filled(bar, CornerRadius::ZERO, to_color(color));
                    }
                    DecorationKind::Glyph(color) if self.glyph_margin && decoration.first_line(text) == line => {
                        let center = pos2(gutter_rect.left() + metrics.glyph_margin_width / 2.0, center_y);
                        gutter_painter.circle_filled(center, metrics.row_height * 0.28, to_color(color));
                    }
                    _ => {}
                }
            }

            let number = match self.config.line_numbers {
                LineNumbers::Off => continue,
                LineNumbers::On => line + 1,
                LineNumbers::Relative if line == primary_head.line => line + 1,
                LineNumbers::Relative => line.abs_diff(primary_head.line),
            };
            let color =
                line_number_color.unwrap_or(if is_cursor_line { theme.active_line_number } else { theme.line_number });
            gutter_painter.text(
                pos2(numbers_right, center_y),
                Align2::RIGHT_CENTER,
                number.to_string(),
                font_id.clone(),
                color,
            );
        }
    }

    fn paint_scrollbar(&self, ui: &Ui, painter: &Painter, metrics: &Metrics, text: &Text, selections: &Selections) {
        let theme = self.theme;
        let track = metrics.scrollbar_rect;
        painter.rect_filled(track, CornerRadius::ZERO, theme.background);
        painter.vline(track.left(), track.y_range(), Stroke::new(1.0, theme.overview_ruler_border));

        if self.config.overview_ruler {
            let row_count = self.row_count().max(1) as f32;
            let y_of_row = |row: usize| track.top() + (row as f32 + 0.5) / row_count * track.height();
            let mark_height = (track.height() / row_count).max(2.0);
            if let Some(display_layout) = self.layout() {
                for (row, display_row) in display_layout.rows().iter().enumerate() {
                    if let (Some(background), Some(_)) = (display_row.background, display_row.marker) {
                        let mark = Rect::from_center_size(
                            pos2(track.center().x, y_of_row(row)),
                            vec2(track.width() - 4.0, mark_height),
                        );
                        painter.rect_filled(mark, CornerRadius::ZERO, to_color(background).gamma_multiply(2.0));
                    }
                }
            }
            for decoration in self.all_decorations() {
                if let Some(color) = decoration.overview_ruler {
                    let y = y_of_row(self.row_of_line(decoration.first_line(text)));
                    let mark = Rect::from_center_size(pos2(track.center().x, y), vec2(track.width() - 4.0, 3.0));
                    painter.rect_filled(mark, CornerRadius::ZERO, to_color(color));
                }
            }
            let find_mark_color = theme.find_match_background.gamma_multiply(1.5);
            for found in self.view.find.matches() {
                let y = y_of_row(self.row_of_line(text.line_of_offset(found.start)));
                painter.hline(track.x_range(), y, Stroke::new(2.0, find_mark_color));
            }
            for selection in selections.iter() {
                let y = y_of_row(self.row_of_line(text.line_of_offset(selection.head)));
                painter.hline(track.x_range(), y, Stroke::new(2.0, theme.cursor.gamma_multiply(0.7)));
            }
        }

        let pointer = ui.input(|input| input.pointer.hover_pos());
        if let Some(thumb) = self.horizontal_scrollbar_thumb(metrics) {
            let is_active = matches!(self.view.drag, Some(Drag::HorizontalScrollbar { .. }))
                || self
                    .horizontal_scrollbar_track(metrics)
                    .is_some_and(|track| pointer.is_some_and(|pos| track.contains(pos)));
            let color = if is_active { theme.scrollbar_thumb_hovered } else { theme.scrollbar_thumb };
            painter.rect_filled(thumb.shrink2(vec2(0.0, 1.0)), CornerRadius::ZERO, color);
        }
        if self.max_scroll_y(metrics) <= 0.0 {
            return;
        }
        let thumb = self.scrollbar_thumb(metrics);
        let is_hovered = ui.input(|input| input.pointer.hover_pos()).is_some_and(|pos| track.contains(pos));
        let is_dragged = matches!(self.view.drag, Some(Drag::VerticalScrollbar { .. }));
        let color = if is_hovered || is_dragged { theme.scrollbar_thumb_hovered } else { theme.scrollbar_thumb };
        painter.rect_filled(thumb.shrink2(vec2(1.0, 0.0)), CornerRadius::ZERO, color);
    }
}

/// VS Code's diagonal stripes where a diff side has no line.
fn paint_padding(painter: &Painter, rect: Rect, color: Color32) {
    const STRIPE_SPACING: f32 = 8.0;
    let clipped = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        clipped.line_segment([pos2(x, rect.bottom()), pos2(x + rect.height(), rect.top())], Stroke::new(1.0, color));
        x += STRIPE_SPACING;
    }
}

fn paint_squiggle(painter: &Painter, left: f32, right: f32, y: f32, color: Color32) {
    const WAVE_WIDTH: f32 = 3.0;
    const WAVE_HEIGHT: f32 = 1.5;
    let mut points = Vec::new();
    let mut x = left;
    let mut is_up = true;
    while x <= right {
        points.push(pos2(x, if is_up { y - WAVE_HEIGHT } else { y + WAVE_HEIGHT }));
        x += WAVE_WIDTH;
        is_up = !is_up;
    }
    if points.len() > 1 {
        painter.add(egui::Shape::line(points, Stroke::new(1.0, color)));
    }
}
