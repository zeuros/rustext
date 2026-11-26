//! Laying out one line for painting: tabs expanded, tokens colored, and the mapping between
//! char columns and x positions (DECISIONS D15).

use std::sync::Arc;

use egui::text::{ByteIndex, CCursor, LayoutJob, LayoutSection, TextFormat, TextWrapping};
use egui::{Color32, FontId, Galley, Stroke, Ui, vec2};
use rustext_core::style::{Rgba, StyledSpan, TextStyle};

use super::{CodeEditor, Metrics};

pub fn to_color(rgba: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(rgba.red, rgba.green, rgba.blue, rgba.alpha)
}

pub fn to_rgba(color: Color32) -> Rgba {
    let [red, green, blue, alpha] = color.to_srgba_unmultiplied();
    Rgba { red, green, blue, alpha }
}

/// One laid-out line: its galley and where each char column lands in it (tabs expanded).
pub(super) struct LineLayout {
    pub galley: Arc<Galley>,
    display_index_of_column: Vec<usize>,
}

impl LineLayout {
    pub fn x_of_column(&self, column: usize) -> f32 {
        let index =
            self.display_index_of_column.get(column).or(self.display_index_of_column.last()).copied().unwrap_or(0);
        self.galley.pos_from_cursor(CCursor::new(index)).min.x
    }

    pub fn column_at_x(&self, x: f32) -> usize {
        let display_index = self.galley.cursor_from_pos(vec2(x, self.galley.size().y / 2.0)).index.0;
        self.display_index_of_column
            .iter()
            .position(|&index| index >= display_index)
            .unwrap_or(self.display_index_of_column.len() - 1)
    }

    pub fn width(&self) -> f32 {
        self.galley.size().x
    }
}

impl CodeEditor<'_> {
    pub(super) fn layout_line(&mut self, ui: &Ui, metrics: &Metrics, line: usize) -> LineLayout {
        /// Lines of bracket depths computed at most per line laid out (keeps far jumps cheap).
        const MAX_BRACKET_DEPTH_LINES: usize = 20_000;
        let text = self.state.text();
        let spans = self.styler.as_deref_mut().map(|styler| styler.style_line(text, line)).unwrap_or_default();
        let content = text.line_content(line);
        let colors = &self.theme.bracket_pair_colors;
        let brackets = &self.state.language.brackets;
        let colorizes = self.config.bracket_pair_colorization && !colors.is_empty();
        let depth = colorizes
            .then(|| self.view.bracket_depths.depth_at(text, brackets, line, MAX_BRACKET_DEPTH_LINES))
            .flatten();
        let bracket_colors: Vec<(usize, Color32)> = match depth {
            Some(depth) => rustext_core::structure::bracket_columns(&content, depth, brackets)
                .into_iter()
                .map(|(column, depth)| (column, colors[depth % colors.len()]))
                .collect(),
            None => Vec::new(),
        };
        self.layout_content(ui, metrics, &content, &spans, &bracket_colors)
    }

    pub(super) fn layout_foreign_line(&mut self, ui: &Ui, metrics: &Metrics, line: usize) -> Option<LineLayout> {
        let foreign = self.foreign.as_mut()?;
        let spans = match foreign.styler.as_deref_mut() {
            Some(styler) => styler.style_line(foreign.text, line),
            None => Vec::new(),
        };
        let content = foreign.text.line_content(line);
        Some(self.layout_content(ui, metrics, &content, &spans, &[]))
    }

    fn layout_content(
        &self,
        ui: &Ui,
        metrics: &Metrics,
        content: &str,
        spans: &[StyledSpan],
        color_overrides: &[(usize, Color32)],
    ) -> LineLayout {
        let tab_size = self.state.config.indentation.tab_size;
        let (job, display_index_of_column) = line_layout_job(
            content,
            spans,
            color_overrides,
            metrics.font_id(),
            self.theme.foreground,
            tab_size,
            self.config.max_rendered_line_chars,
        );
        let galley = ui.ctx().fonts_mut(|fonts| fonts.layout_job(job));
        LineLayout { galley, display_index_of_column }
    }
}

/// Builds the layout job of a line: tabs expanded to spaces, spans colored, then the colors of
/// `color_overrides` (sorted by column) on their chars. Returns the job and, for each char
/// column (plus the end), its char index in the job's text.
pub(super) fn line_layout_job(
    content: &str,
    spans: &[StyledSpan],
    color_overrides: &[(usize, Color32)],
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
    let tab_size = tab_size.max(1);

    for (column, character) in content.chars().enumerate().take(max_chars) {
        display_index_of_column.push(display_len);
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
        let byte_start = job.text.len();
        if character == '\t' {
            let spaces = tab_size - display_len % tab_size;
            job.text.extend(std::iter::repeat_n(' ', spaces));
            display_len += spaces;
        } else {
            job.text.push(character);
            display_len += 1;
        }
        match job.sections.last_mut() {
            Some(section) if section.format == format => section.byte_range.end = ByteIndex(job.text.len()),
            _ => job.sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: ByteIndex(byte_start)..ByteIndex(job.text.len()),
                format,
            }),
        }
    }
    display_index_of_column.push(display_len);
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
