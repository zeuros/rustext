//! Folding: the folded blocks of a view, the fold range a line starts, the fold actions and
//! the rows of a folded text. Only the visible lines and the folded blocks are read, so a
//! keystroke in a huge file stays cheap; whole-text work happens only on fold all.

use rustext_core::EditedLines;
use rustext_core::display::{DisplayLayout, DisplayRow, RowContent};
use rustext_core::fold_markers::{region_fold_range, region_fold_ranges};
use rustext_core::indentation::Indentation;
use rustext_core::motion::HiddenLines;
use rustext_core::structure::{FoldRange, fold_range_at, indentation_fold_ranges};
use rustext_core::style::LineStyler;
use rustext_core::{Selections, Text};

use super::{CodeEditor, FoldingStrategy};
use crate::keymap::Action;

/// A folded block: its header line and last hidden line, shifted by edits above it.
/// `needs_recompute` when an edit touched it (its extent may have changed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FoldedBlock {
    header_line: usize,
    last_hidden_line: usize,
    needs_recompute: bool,
    /// Where the header may be instead, after an edit across its line (Enter before its text
    /// moves it down): tried when `header_line` no longer starts a fold.
    other_header_line: Option<usize>,
}

/// Per-view folding state and what is derived from it.
#[derive(Clone, Debug, Default)]
pub struct FoldState {
    /// Sorted by header line, one per header.
    blocks: Vec<FoldedBlock>,
    /// The rows when something is folded or wrapped, and the line count, folded ranges and wrap
    /// generation they were built for.
    /// What the layout was built from: line count, folded ranges, wrap generation, code lens
    /// lines.
    layout_key: Option<(usize, Vec<FoldRange>, u64, Vec<usize>)>,
    pub(super) layout: Option<DisplayLayout>,
}

impl FoldState {
    pub fn folded_header_count(&self) -> usize {
        self.blocks.len()
    }

    /// The folded blocks as last computed, sorted by header line.
    pub fn folded_ranges(&self) -> Vec<FoldRange> {
        let range = |block: &FoldedBlock| FoldRange {
            header_line: block.header_line,
            last_hidden_line: block.last_hidden_line,
        };
        self.blocks.iter().map(range).collect()
    }

    /// Header lines of the folded blocks, sorted.
    pub fn folded_header_lines(&self) -> Vec<usize> {
        self.blocks.iter().map(|block| block.header_line).collect()
    }

    /// Folds the region whose header is `header_line` (kept only if a fold range starts there).
    pub fn fold_line(&mut self, header_line: usize) {
        if let Err(index) = self.blocks.binary_search_by_key(&header_line, |block| block.header_line) {
            let block = FoldedBlock {
                header_line,
                last_hidden_line: header_line,
                needs_recompute: true,
                other_header_line: None,
            };
            self.blocks.insert(index, block);
        }
    }

    fn is_folded(&self, header_line: usize) -> bool {
        self.blocks.binary_search_by_key(&header_line, |block| block.header_line).is_ok()
    }

    fn unfold(&mut self, header_line: usize) {
        if let Ok(index) = self.blocks.binary_search_by_key(&header_line, |block| block.header_line) {
            self.blocks.remove(index);
        }
    }

    /// Blocks after an edit: those below it shift, those it touched (or the line right after
    /// them) are recomputed; a header the edit went across is looked for on the edit's first
    /// and last lines.
    pub(super) fn follow_edit(&mut self, edited: &EditedLines) {
        for block in &mut self.blocks {
            let touches_block =
                edited.first_line <= block.last_hidden_line + 1 && edited.old_last_line >= block.header_line;
            let (header_line, other_header_line) = match edited.map_line(block.header_line) {
                Some(line) => (line, block.other_header_line),
                None => {
                    (edited.first_line, (edited.new_last_line != edited.first_line).then_some(edited.new_last_line))
                }
            };
            let last_hidden_line =
                edited.map_line(block.last_hidden_line).unwrap_or(edited.new_last_line.max(header_line));
            *block = FoldedBlock {
                header_line,
                last_hidden_line,
                needs_recompute: block.needs_recompute || touches_block,
                other_header_line,
            };
        }
        self.blocks.dedup_by_key(|block| block.header_line);
    }
}

/// The lines folded ranges hide, for motions.
fn hidden_lines_of(folded: &[FoldRange]) -> HiddenLines {
    HiddenLines::new(folded.iter().map(|range| (range.first_hidden_line(), range.last_hidden_line)).collect())
}

/// The fold range `line` starts: a `#region` marked in a comment (D33), else from the
/// styler's syntax tree when it has one and the strategy allows, else from indentation.
fn fold_range_of<'styler>(
    styler: Option<&mut (dyn LineStyler + 'styler)>,
    strategy: FoldingStrategy,
    text: &Text,
    indentation: &Indentation,
    line: usize,
) -> Option<FoldRange> {
    if let Some(region) = region_fold_range(text, line) {
        return Some(region);
    }
    match styler.filter(|styler| strategy == FoldingStrategy::Auto && styler.folds_by_syntax()) {
        Some(styler) => styler.syntax_fold_range(text, line),
        None => fold_range_at(text, indentation, line),
    }
}

impl CodeEditor<'_> {
    fn folds_by_syntax(&self) -> bool {
        self.config.folding_strategy == FoldingStrategy::Auto
            && self.styler.as_deref().is_some_and(|styler| styler.folds_by_syntax())
    }

    fn fold_range_at_line(&mut self, line: usize) -> Option<FoldRange> {
        let (text, indentation) = (self.state.text(), &self.state.config.indentation);
        fold_range_of(self.styler.as_deref_mut(), self.config.folding_strategy, text, indentation, line)
    }

    /// The folded ranges: untouched blocks as they are, touched ones computed again (and
    /// dropped when their header no longer starts a fold).
    fn folded_ranges(&mut self) -> Vec<FoldRange> {
        let text = self.state.text().clone();
        let indentation = self.state.config.indentation;
        let strategy = self.config.folding_strategy;
        let styler = &mut self.styler;
        let mut range_at = |line: usize| fold_range_of(styler.as_deref_mut(), strategy, &text, &indentation, line);
        let mut ranges = Vec::with_capacity(self.view.folds.blocks.len());
        self.view.folds.blocks.retain_mut(|block| {
            if !block.needs_recompute {
                ranges.push(FoldRange { header_line: block.header_line, last_hidden_line: block.last_hidden_line });
                return true;
            }
            let range = range_at(block.header_line).or_else(|| block.other_header_line.and_then(&mut range_at));
            match range {
                Some(range) => {
                    *block = FoldedBlock {
                        header_line: range.header_line,
                        last_hidden_line: range.last_hidden_line,
                        needs_recompute: false,
                        other_header_line: None,
                    };
                    ranges.push(range);
                    true
                }
                None => false,
            }
        });
        ranges
    }

    /// The folded ranges once those hiding a cursor are unfolded.
    fn folded_ranges_showing_cursors(&mut self, text: &Text) -> Vec<FoldRange> {
        let cursor_lines: Vec<usize> =
            self.state.selections().iter().map(|selection| text.line_of_offset(selection.head)).collect();
        for range in self.folded_ranges() {
            if cursor_lines.iter().any(|&line| range.hides(line)) {
                self.unfold(range.header_line);
            }
        }
        self.folded_ranges()
    }

    /// The folded line whose end `pos` is past: on its `⋯` label, or anywhere past it with
    /// `unfold_on_click_after_end_of_line`. A click there unfolds it.
    pub(super) fn folded_line_clicked_past_its_end(
        &mut self,
        ui: &egui::Ui,
        metrics: &super::Metrics,
        pos: egui::Pos2,
    ) -> Option<usize> {
        const LABEL_CHARS: f32 = 3.0;
        let (line, start_column, end_column) = self.row_segment(self.row_at_y(metrics, pos.y))?;
        if !self.is_folded(line) || end_column < self.state.text().line_len(line) {
            return None;
        }
        let line_width = self.layout_segment(ui, metrics, line, start_column, end_column).width();
        let x = pos.x - self.text_left(metrics);
        let is_on_label = x <= line_width + metrics.char_width * LABEL_CHARS;
        (x > line_width && (is_on_label || self.config.unfold_on_click_after_end_of_line)).then_some(line)
    }

    /// Whether `line` starts a fold range (a local check, fine for every visible line).
    pub(super) fn is_fold_header(&mut self, line: usize) -> bool {
        self.fold_range_at_line(line).is_some()
    }

    pub(super) fn is_folded(&self, header_line: usize) -> bool {
        self.view.folds.is_folded(header_line)
    }

    pub(super) fn toggle_fold(&mut self, header_line: usize) {
        if self.is_folded(header_line) {
            self.unfold(header_line);
        } else if self.is_fold_header(header_line) {
            self.view.folds.fold_line(header_line);
        }
    }

    fn unfold(&mut self, header_line: usize) {
        self.view.folds.unfold(header_line);
    }

    /// Runs the folding actions; returns the actions it doesn't handle.
    pub(super) fn run_fold_action(&mut self, action: Action) -> Option<Action> {
        let text = self.state.text().clone();
        let cursor_line = text.line_of_offset(self.state.selections().primary().head);
        let holds_cursor = |range: &FoldRange| range.header_line == cursor_line || range.hides(cursor_line);
        match action {
            Action::Fold => {
                // The innermost unfolded range holding the cursor: headers looked up from it.
                let header = (0..=cursor_line).rev().find(|&line| {
                    !self.is_folded(line) && self.fold_range_at_line(line).is_some_and(|range| holds_cursor(&range))
                });
                // A cursor the fold would hide moves to the end of its header, as in VS Code
                // (else the hidden cursor would unfold it again).
                if let Some(range) = header.and_then(|header| self.fold_range_at_line(header)) {
                    self.toggle_fold(range.header_line);
                    if range.hides(cursor_line) {
                        let header_end = text.line_content_end(range.header_line);
                        self.execute_tracked(rustext_core::Command::SetSelections(Selections::cursor(header_end)));
                    }
                }
            }
            Action::Unfold => {
                let innermost =
                    self.folded_ranges().into_iter().filter(holds_cursor).max_by_key(|range| range.header_line);
                if let Some(range) = innermost {
                    self.unfold(range.header_line);
                }
            }
            Action::FoldAll => {
                let mut ranges: Vec<FoldRange> = if self.folds_by_syntax() {
                    (0..text.line_count()).filter_map(|line| self.fold_range_at_line(line)).collect()
                } else {
                    indentation_fold_ranges(&text, &self.state.config.indentation)
                };
                ranges.extend(region_fold_ranges(&text));
                ranges.sort_by_key(|range| range.header_line);
                ranges.dedup_by_key(|range| range.header_line);
                self.view.folds.blocks = ranges
                    .iter()
                    .map(|range| FoldedBlock {
                        header_line: range.header_line,
                        last_hidden_line: range.last_hidden_line,
                        needs_recompute: false,
                        other_header_line: None,
                    })
                    .collect();
            }
            Action::UnfoldAll => self.view.folds.blocks.clear(),
            other => return Some(other),
        }
        None
    }

    /// Unfolds what hides a cursor, wraps lines, then (when folds or wraps changed) rebuilds
    /// the rows.
    pub(super) fn refresh_rows(&mut self, metrics: &super::Metrics) {
        let text = self.state.text().clone();
        let wrap_width_columns = self.wrap_width_columns(metrics);
        self.view.wraps.ensure(&text, wrap_width_columns, self.state.config.indentation.tab_size);
        let code_lens_lines = self.code_lens_lines();
        let needs_layout = self.config.folding || self.view.wraps.is_wrapping() || !code_lens_lines.is_empty();
        if self.display_layout.is_some() {
            // The layout's builder leaves the folded rows out; motions still skip their lines.
            let folded = if self.folds() { self.folded_ranges_showing_cursors(&text) } else { Vec::new() };
            self.view.folds.layout = None;
            self.state.hidden_lines = hidden_lines_of(&folded);
            return;
        }
        if !needs_layout {
            self.view.folds.layout = None;
            self.state.hidden_lines = HiddenLines::default();
            return;
        }
        let folded = if self.config.folding { self.folded_ranges_showing_cursors(&text) } else { Vec::new() };
        let folds = &mut self.view.folds;
        if folded.is_empty() && !self.view.wraps.is_wrapping() && code_lens_lines.is_empty() {
            folds.layout = None;
            folds.layout_key = None;
            self.state.hidden_lines = HiddenLines::default();
            return;
        }
        let key = (text.line_count(), folded, self.view.wraps.generation(), code_lens_lines);
        if folds.layout.is_some() && folds.layout_key.as_ref() == Some(&key) {
            return;
        }
        let (folded, code_lens_lines) = (key.1.clone(), &key.3);

        let hidden_lines = hidden_lines_of(&folded);
        let wraps = &self.view.wraps;
        let rows = hidden_lines
            .shown_lines(text.line_count())
            .flat_map(|line| {
                let continuations = wraps
                    .points(line)
                    .iter()
                    .map(move |&start_column| DisplayRow::new(RowContent::Wrapped { line, start_column }));
                let code_lens =
                    code_lens_lines.binary_search(&line).ok().map(|_| DisplayRow::new(RowContent::CodeLens(line)));
                code_lens
                    .into_iter()
                    .chain(std::iter::once(DisplayRow::new(RowContent::Line(line))))
                    .chain(continuations)
            })
            .collect();
        folds.layout = Some(DisplayLayout::new(rows, text.line_count()));
        folds.layout_key = Some(key);
        self.state.hidden_lines = hidden_lines;
    }
}

/// Lines a fold placeholder `⋯` follows: the folded headers, sorted.
pub(super) fn folded_header_lines(folds: &FoldState) -> Vec<usize> {
    folds.blocks.iter().map(|block| block.header_line).collect()
}
