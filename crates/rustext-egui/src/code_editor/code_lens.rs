//! Code lenses: labels in a row above their line, aligned with its indentation, separated by
//! ` | `. The application's come first, then the language services' (asked once the text
//! rests). A click on the application's is reported, the application decides what it does
//! (D25); one on the services' lists its places or hands its command to the application.

use egui::{Align2, FontId, Painter, pos2};
use rustext_core::services::{CodeLens, CodeLensAction, Reply, ServiceCommand};
use rustext_core::{Bias, ChangeSet, Text};

use super::list_at_cursor::ListAtCursor;
use super::{CodeEditor, Metrics};

/// Code lens text, smaller than the code's.
const CODE_LENS_FONT_SCALE: f32 = 0.85;
const SEPARATOR: &str = " | ";

/// How long the text rests unchanged before the services are asked for its lenses again.
const CODE_LENS_REST_SECONDS: f64 = 0.3;

/// The language services' code lenses, their lines following edits until the next answer.
#[derive(Clone, Debug, Default)]
pub struct ServiceCodeLenses {
    lenses: Vec<CodeLens>,
    /// The char offset of each lens's line start, mapped through edits.
    line_starts: Vec<usize>,
    /// The text revision the lenses were asked for.
    answered_revision: Option<u64>,
    /// The text revision last seen and when it was first seen (seconds), to ask once it rests.
    revision_seen_at: Option<(u64, f64)>,
}

impl ServiceCodeLenses {
    pub fn lenses(&self) -> &[CodeLens] {
        &self.lenses
    }

    fn set(&mut self, lenses: Vec<CodeLens>, text: &Text) {
        self.line_starts = lenses.iter().map(|lens| text.line_start(lens.line.min(text.last_line()))).collect();
        self.lenses = lenses;
    }

    /// Each lens follows its line's text: a line break typed at the line's start moves it down.
    pub(super) fn map(&mut self, changes: &ChangeSet) {
        for line_start in &mut self.line_starts {
            *line_start = changes.map_offset(*line_start, Bias::After);
        }
    }

    /// The lines of the lenses in `text`, after their line starts were mapped.
    pub(super) fn place_on_lines(&mut self, text: &Text) {
        for (lens, &line_start) in self.lenses.iter_mut().zip(&self.line_starts) {
            lens.line = text.line_of_offset(line_start);
        }
    }
}

impl CodeEditor<'_> {
    /// The application's code lenses, then the services': `MouseTarget::CodeLens` indexes them.
    fn all_code_lenses(&self) -> impl Iterator<Item = &CodeLens> {
        self.code_lenses.iter().chain(self.view.service_code_lenses.lenses())
    }

    pub(super) fn code_lens(&self, index: usize) -> Option<&CodeLens> {
        self.all_code_lenses().nth(index)
    }

    /// Asks the services for the lenses once the text rested since it changed (at once the
    /// first time). Returns whether an answer is awaited.
    pub(super) fn refresh_code_lenses(&mut self, now: f64) -> bool {
        let revision = self.state.text_revision();
        let lenses = &mut self.view.service_code_lenses;
        let Some(services) = self.services.as_deref_mut().filter(|_| self.config.code_lens) else {
            *lenses = ServiceCodeLenses::default();
            return false;
        };
        if lenses.answered_revision == Some(revision) {
            return false;
        }
        if lenses.revision_seen_at.is_none_or(|(seen_revision, _)| seen_revision != revision) {
            lenses.revision_seen_at = Some((revision, now));
        }
        let seen_at = lenses.revision_seen_at.map_or(now, |(_, seen_at)| seen_at);
        if lenses.answered_revision.is_some() && now - seen_at < CODE_LENS_REST_SECONDS {
            return true;
        }
        match services.code_lenses(self.state.text(), revision) {
            Reply::Ready(answer) => {
                lenses.set(answer, self.state.text());
                lenses.answered_revision = Some(revision);
                false
            }
            Reply::Pending => true,
        }
    }

    /// What a click on lens `index` does: the application's is reported (its index in
    /// `code_lenses`); a services' one lists its places, or returns its command.
    pub(super) fn click_code_lens(&mut self, index: usize) -> (Option<usize>, Option<ServiceCommand>) {
        if index < self.code_lenses.len() {
            return (Some(index), None);
        }
        let Some(lens) = self.code_lens(index).cloned() else { return (None, None) };
        match lens.action {
            CodeLensAction::Report => (None, None),
            CodeLensAction::Run(command) => (None, Some(command)),
            CodeLensAction::ShowLocations(locations) => {
                let revision = self.state.text_revision();
                let anchor = self.state.text().line_start(lens.line);
                self.view.references = ListAtCursor::asked(revision, anchor);
                self.view.references.answer(locations, 0, revision);
                (None, None)
            }
        }
    }

    /// The lines that have code lenses, sorted, each once.
    pub(super) fn code_lens_lines(&self) -> Vec<usize> {
        let line_count = self.state.text().line_count();
        let mut lines: Vec<usize> =
            self.all_code_lenses().map(|lens| lens.line).filter(|&line| line < line_count).collect();
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
        for (index, lens) in self.all_code_lenses().enumerate().filter(|(_, lens)| lens.line == line) {
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
                self.code_lens(index).map_or("", |lens| lens.label.as_str()),
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
