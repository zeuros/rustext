//! Painting the visible rows, the gutter, cursors and the scrollbar.

use egui::{Align2, Color32, CornerRadius, Painter, Rect, Stroke, StrokeKind, Ui, pos2, vec2};
use rustext_core::decoration::{Decoration, DecorationKind};
use rustext_core::display::RowContent;
use rustext_core::{Position, Selection, Selections, Text};

use super::layout::{LineLayout, to_color};
use super::{CodeEditor, CurrentLineHighlight, CursorStyle, Drag, LineNumbers, Metrics};
use crate::theme::EditorTheme;

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

        let mut rows = Vec::new();
        for row in self.visible_rows(metrics) {
            let content = self.row_content(row);
            let layout = match content {
                RowContent::Line(line) => Some(self.layout_line(ui, metrics, line)),
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
            if let (Some(background), _) = self.row_style(painted.row) {
                text_painter.rect_filled(row_rect, CornerRadius::ZERO, to_color(background));
            }
            match (painted.content, &painted.layout) {
                (RowContent::Line(line), Some(layout)) => {
                    let paints_current_line = matches!(
                        self.config.current_line_highlight,
                        CurrentLineHighlight::Line | CurrentLineHighlight::All
                    );
                    if paints_current_line && highlights_current_line && cursor_lines.contains(&line) {
                        text_painter.rect_filled(row_rect, CornerRadius::ZERO, theme.current_line_background);
                    }
                    self.paint_line_decorations(&text_painter, metrics, &text, line, layout, top, self.decorations);
                    self.paint_selections(&text_painter, metrics, &text, line, layout, top, &selections, has_focus);
                    self.paint_galley(&text_painter, metrics, layout, top);
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
        for column in &self.config.rulers {
            let x = text_left + *column as f32 * metrics.char_width;
            text_painter.vline(x, metrics.text_rect.y_range(), Stroke::new(1.0, theme.ruler));
        }

        let now = ui.input(|input| input.time);
        let blink_on = !self.config.cursor_blinking || ((now - self.view.last_interaction_time) % 1.0) < 0.55;
        let layout_of_line = |line: usize| {
            rows.iter()
                .find(|painted| painted.content == RowContent::Line(line))
                .and_then(|painted| painted.layout.as_ref().map(|layout| (painted.row, layout)))
        };
        if has_focus && blink_on {
            for selection in selections.iter() {
                let head = text.position(selection.head);
                if let Some((row, layout)) = layout_of_line(head.line) {
                    self.paint_cursor(&text_painter, metrics, row, layout, head, &theme);
                }
            }
        }
        if has_focus {
            if self.config.cursor_blinking {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
            }
            if let Some((row, layout)) = layout_of_line(primary_head.line) {
                self.paint_ime(ui, &text_painter, metrics, row, layout, primary_head, &theme);
            }
        }
        if self.styler.as_deref().is_some_and(|styler| styler.is_catching_up()) {
            ui.ctx().request_repaint();
        }

        let paints_gutter_line =
            matches!(self.config.current_line_highlight, CurrentLineHighlight::Gutter | CurrentLineHighlight::All);
        self.paint_gutter(
            &painter,
            metrics,
            &text,
            &rows,
            &cursor_lines,
            primary_head,
            paints_gutter_line && highlights_current_line,
        );
        self.paint_scrollbar(ui, &painter, metrics, &text, &selections);
    }

    fn paint_galley(&self, painter: &Painter, metrics: &Metrics, layout: &LineLayout, top: f32) {
        let text_top = top + (metrics.row_height - layout.galley.size().y) / 2.0;
        painter.galley(pos2(self.text_left(metrics), text_top), layout.galley.clone(), self.theme.foreground);
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
            let continues_past_line = selection.end() > line_end;
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
            match decoration.kind {
                DecorationKind::WholeLineBackground(color) => {
                    let row_rect = Rect::from_min_max(
                        pos2(metrics.text_rect.left(), top),
                        pos2(metrics.text_rect.right(), bottom),
                    );
                    painter.rect_filled(row_rect, CornerRadius::ZERO, to_color(color));
                }
                DecorationKind::Background(color) => {
                    painter.rect_filled(
                        Rect::from_min_max(pos2(left, top), pos2(right, bottom)),
                        CornerRadius::ZERO,
                        to_color(color),
                    );
                }
                DecorationKind::Border(color) => {
                    let rect = Rect::from_min_max(pos2(left, top), pos2(right.max(left + 2.0), bottom));
                    painter.rect_stroke(
                        rect,
                        CornerRadius::same(2),
                        Stroke::new(1.0, to_color(color)),
                        StrokeKind::Inside,
                    );
                }
                DecorationKind::Squiggle(color) => {
                    paint_squiggle(painter, left, right.max(left + metrics.char_width), bottom - 2.0, to_color(color));
                }
                DecorationKind::Underline(color) => {
                    painter.hline(left..=right, bottom - 1.5, Stroke::new(1.0, to_color(color)));
                }
                DecorationKind::Glyph(_) | DecorationKind::LineNumberColor(_) => {}
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

    #[allow(clippy::too_many_arguments)]
    fn paint_gutter(
        &self,
        painter: &Painter,
        metrics: &Metrics,
        text: &Text,
        rows: &[PaintedRow],
        cursor_lines: &[usize],
        primary_head: Position,
        highlights_current_line: bool,
    ) {
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
                RowContent::Padding | RowContent::Foreign(_) => continue,
            };
            let is_cursor_line = cursor_lines.contains(&line);
            if highlights_current_line && is_cursor_line {
                gutter_painter.rect_filled(row_rect, CornerRadius::ZERO, theme.current_line_background);
            }
            let mut line_number_color = None;
            for decoration in self.decorations.iter().filter(|decoration| decoration.covers_line(text, line)) {
                match decoration.kind {
                    DecorationKind::WholeLineBackground(color) => {
                        gutter_painter.rect_filled(row_rect, CornerRadius::ZERO, to_color(color));
                    }
                    DecorationKind::LineNumberColor(color) => line_number_color = Some(to_color(color)),
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
            if let Some(display_layout) = self.display_layout {
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
            for decoration in self.decorations {
                if let Some(color) = decoration.overview_ruler {
                    let y = y_of_row(self.row_of_line(decoration.first_line(text)));
                    let mark = Rect::from_center_size(pos2(track.center().x, y), vec2(track.width() - 4.0, 3.0));
                    painter.rect_filled(mark, CornerRadius::ZERO, to_color(color));
                }
            }
            for selection in selections.iter() {
                let y = y_of_row(self.row_of_line(text.line_of_offset(selection.head)));
                painter.hline(track.x_range(), y, Stroke::new(2.0, theme.cursor.gamma_multiply(0.7)));
            }
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
