//! Laying out one line for painting: tabs expanded, tokens colored, and the mapping between
//! char columns and x positions (DECISIONS D15).

use std::sync::Arc;

use egui::text::{ByteIndex, CCursor, LayoutJob, LayoutSection, TextFormat, TextWrapping};
use egui::{Color32, FontId, Galley, Stroke, Ui, vec2};
use rustext_core::structure::BracketScanner;
use rustext_core::style::{Rgba, StyledSpan, TextStyle};

use super::{CodeEditor, Metrics};

pub fn to_color(rgba: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(rgba.red, rgba.green, rgba.blue, rgba.alpha)
}

pub fn to_rgba(color: Color32) -> Rgba {
    let [red, green, blue, alpha] = color.to_srgba_unmultiplied();
    Rgba { red, green, blue, alpha }
}

/// One laid-out line, or segment of a wrapped line from `first_column`: its galley and where
/// each char column lands in it (tabs expanded). Columns are the line's; columns outside the
/// segment clamp to its edges, so range painters need no wrap logic.
pub(super) struct LineLayout {
    pub galley: Arc<Galley>,
    display_index_of_column: Vec<usize>,
    pub first_column: usize,
    /// Space before the text: a wrapped line's continuation rows are indented like the line.
    pub indent_px: f32,
}

impl LineLayout {
    pub fn x_of_column(&self, column: usize) -> f32 {
        let local_column = column.saturating_sub(self.first_column);
        let index = self
            .display_index_of_column
            .get(local_column)
            .or(self.display_index_of_column.last())
            .copied()
            .unwrap_or(0);
        self.indent_px + self.galley.pos_from_cursor(CCursor::new(index)).min.x
    }

    pub fn column_at_x(&self, x: f32) -> usize {
        let display_index = self.galley.cursor_from_pos(vec2(x - self.indent_px, self.galley.size().y / 2.0)).index.0;
        let local_column = self
            .display_index_of_column
            .iter()
            .position(|&index| index >= display_index)
            .unwrap_or(self.display_index_of_column.len() - 1);
        self.first_column + local_column
    }

    /// The column after the segment's last char.
    pub fn end_column(&self) -> usize {
        self.first_column + self.display_index_of_column.len() - 1
    }

    pub fn width(&self) -> f32 {
        self.indent_px + self.galley.size().x
    }
}

impl CodeEditor<'_> {
    /// Injected texts of a line (from decorations and the ghost text), sorted by column.
    fn injections_of_line(&self, line: usize) -> Vec<Injection> {
        let text = self.state.text();
        let (line_start, line_end) = (text.line_start(line), text.line_content_end(line));
        let mut injections: Vec<Injection> = self
            .all_decorations()
            .filter(|decoration| (line_start..=line_end).contains(&decoration.start))
            .filter_map(|decoration| match &decoration.kind {
                rustext_core::decoration::DecorationKind::InjectedText { text, color } => Some(Injection {
                    column: decoration.start - line_start,
                    text: text.clone(),
                    color: to_color(*color),
                }),
                _ => None,
            })
            .collect();
        if let Some(ghost) = self.view.ghost_text.as_ref().filter(|ghost| text.line_of_offset(ghost.offset) == line) {
            let first_line = ghost.text.lines().next().unwrap_or_default();
            let shown = if ghost.text.contains('\n') { format!("{first_line} …") } else { first_line.to_string() };
            injections.push(Injection {
                column: ghost.offset - line_start,
                text: shown,
                color: self.theme.hint.gamma_multiply(0.8),
            });
        }
        injections.sort_by_key(|injection| injection.column);
        injections
    }

    /// The whole line (or its first segment's columns onward when it wraps, see `layout_segment`).
    pub(super) fn layout_line(&mut self, ui: &Ui, metrics: &Metrics, line: usize) -> LineLayout {
        self.layout_segment(ui, metrics, line, 0, usize::MAX)
    }

    /// Columns `start_column..end_column` of a line (clamped to the line).
    pub(super) fn layout_segment(
        &mut self,
        ui: &Ui,
        metrics: &Metrics,
        line: usize,
        start_column: usize,
        end_column: usize,
    ) -> LineLayout {
        let text = self.state.text();
        let spans = self.styler.as_deref_mut().map(|styler| styler.style_line(text, line)).unwrap_or_default();
        let content = text.line_content(line);
        let colors = &self.theme.bracket_pair_colors;
        let colorizes = self.config.bracket_pair_colorization && !colors.is_empty();
        let mut scanner = BracketScanner::new(&self.state.language, self.styler.as_deref_mut());
        let line_start = colorizes
            .then(|| self.view.bracket_line_starts.state_at(text, &mut scanner, line, &mut self.bracket_budget_lines))
            .flatten();
        let bracket_colors: Vec<(usize, Color32)> = match line_start {
            Some(line_start) => rustext_core::structure::bracket_columns(text, line, line_start, &mut scanner)
                .into_iter()
                .map(|(column, depth)| (column, colors[depth % colors.len()]))
                .collect(),
            None => Vec::new(),
        };
        let line_len = content.chars().count();
        let segment_end = end_column.min(line_len);
        let is_last_segment = segment_end == line_len;
        let injections: Vec<Injection> = self
            .injections_of_line(line)
            .into_iter()
            .filter(|injection| injection.column >= start_column && (injection.column < segment_end || is_last_segment))
            .map(|injection| Injection { column: injection.column - start_column, ..injection })
            .collect();
        if start_column == 0 && end_column >= line_len {
            return self.layout_content(ui, metrics, &content, &spans, &bracket_colors, &injections);
        }
        let segment: String =
            content.chars().skip(start_column).take(end_column.saturating_sub(start_column)).collect();
        let segment_end = start_column + segment.chars().count();
        let segment_spans: Vec<StyledSpan> = spans
            .iter()
            .filter(|span| span.end_column > start_column && span.start_column < segment_end)
            .map(|span| StyledSpan {
                start_column: span.start_column.max(start_column) - start_column,
                end_column: span.end_column.min(segment_end) - start_column,
                style: span.style,
            })
            .collect();
        let segment_colors: Vec<(usize, Color32)> = bracket_colors
            .iter()
            .filter(|(column, _)| (start_column..segment_end).contains(column))
            .map(|&(column, color)| (column - start_column, color))
            .collect();
        let mut layout = self.layout_content(ui, metrics, &segment, &segment_spans, &segment_colors, &injections);
        layout.first_column = start_column;
        if start_column > 0 {
            let indent_columns = rustext_core::wrap::continuation_indent(
                &content,
                self.view.wraps.width_columns(),
                self.state.config.indentation.tab_size,
            );
            layout.indent_px = indent_columns as f32 * metrics.char_width;
        }
        layout
    }

    pub(super) fn layout_foreign_line(&mut self, ui: &Ui, metrics: &Metrics, line: usize) -> Option<LineLayout> {
        let foreign = self.foreign.as_mut()?;
        let spans = match foreign.styler.as_deref_mut() {
            Some(styler) => styler.style_line(foreign.text, line),
            None => Vec::new(),
        };
        let content = foreign.text.line_content(line);
        Some(self.layout_content(ui, metrics, &content, &spans, &[], &[]))
    }

    fn layout_content(
        &self,
        ui: &Ui,
        metrics: &Metrics,
        content: &str,
        spans: &[StyledSpan],
        color_overrides: &[(usize, Color32)],
        injections: &[Injection],
    ) -> LineLayout {
        let tab_size = self.state.config.indentation.tab_size;
        let (job, display_index_of_column) = line_layout_job(
            content,
            spans,
            color_overrides,
            injections,
            metrics.font_id(),
            self.theme.foreground,
            tab_size,
            self.config.max_rendered_line_chars,
        );
        let galley = ui.ctx().fonts_mut(|fonts| fonts.layout_job(job));
        LineLayout { galley, display_index_of_column, first_column: 0, indent_px: 0.0 }
    }
}

/// Text shown before a column without being part of the line (inlay hints, ghost text).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Injection {
    pub column: usize,
    pub text: String,
    pub color: Color32,
}

/// Appends `piece` to the job in `format`, merging with the last section when it has the same.
fn append_piece(job: &mut LayoutJob, piece: &str, format: TextFormat) {
    let byte_start = job.text.len();
    job.text.push_str(piece);
    match job.sections.last_mut() {
        Some(section) if section.format == format => section.byte_range.end = ByteIndex(job.text.len()),
        _ => job.sections.push(LayoutSection {
            leading_space: 0.0,
            byte_range: ByteIndex(byte_start)..ByteIndex(job.text.len()),
            format,
        }),
    }
}

/// Builds the layout job of a line: tabs expanded to spaces, spans colored, then the colors of
/// `color_overrides` (sorted by column) on their chars, and `injections` (sorted by column)
/// before their column. Returns the job and, for each char column (plus the end), its char
/// index in the job's text, before any injection at that column (the cursor stays before it).
#[allow(clippy::too_many_arguments)]
pub(super) fn line_layout_job(
    content: &str,
    spans: &[StyledSpan],
    color_overrides: &[(usize, Color32)],
    injections: &[Injection],
    font_id: FontId,
    default_color: Color32,
    tab_size: usize,
    max_chars: usize,
) -> (LayoutJob, Vec<usize>) {
    let default_format = TextFormat { font_id: font_id.clone(), color: default_color, ..TextFormat::default() };
    let mut job = LayoutJob { wrap: TextWrapping::no_max_width(), break_on_newline: false, ..LayoutJob::default() };
    let mut display_index_of_column = Vec::with_capacity(content.len() + 1);
    let mut display_len = 0;
    let mut span_index = 0;
    let mut injection_index = 0;
    let tab_size = tab_size.max(1);
    let mut append_injections = |job: &mut LayoutJob, display_len: &mut usize, column: usize| {
        while let Some(injection) = injections.get(injection_index).filter(|injection| injection.column <= column) {
            let format =
                TextFormat { font_id: font_id.clone(), color: injection.color, italics: true, ..TextFormat::default() };
            append_piece(job, &injection.text, format);
            *display_len += injection.text.chars().count();
            injection_index += 1;
        }
    };

    for (column, character) in content.chars().enumerate().take(max_chars) {
        display_index_of_column.push(display_len);
        append_injections(&mut job, &mut display_len, column);
        while spans.get(span_index).is_some_and(|span| span.end_column <= column) {
            span_index += 1;
        }
        let mut format = match spans.get(span_index).filter(|span| span.start_column <= column) {
            Some(span) => text_format(&span.style, &font_id, default_color),
            None => default_format.clone(),
        };
        if let Ok(index) = color_overrides.binary_search_by_key(&column, |(override_column, _)| *override_column) {
            format.color = color_overrides[index].1;
        }
        if character == '\t' {
            let spaces = tab_size - display_len % tab_size;
            append_piece(&mut job, &" ".repeat(spaces), format);
            display_len += spaces;
        } else {
            append_piece(&mut job, character.encode_utf8(&mut [0; 4]), format);
            display_len += 1;
        }
    }
    display_index_of_column.push(display_len);
    append_injections(&mut job, &mut display_len, usize::MAX);
    if job.sections.is_empty() {
        job.sections.push(LayoutSection {
            leading_space: 0.0,
            byte_range: ByteIndex(0)..ByteIndex(0),
            format: default_format,
        });
    }
    (job, display_index_of_column)
}

fn text_format(style: &TextStyle, font_id: &FontId, default_color: Color32) -> TextFormat {
    let color = style.foreground.map(to_color).unwrap_or(default_color);
    TextFormat {
        font_id: font_id.clone(),
        color,
        background: style.background.map(to_color).unwrap_or(Color32::TRANSPARENT),
        italics: style.italic,
        underline: if style.underline { Stroke::new(1.0, color) } else { Stroke::NONE },
        strikethrough: if style.strikethrough { Stroke::new(1.0, color) } else { Stroke::NONE },
        ..TextFormat::default()
    }
}
