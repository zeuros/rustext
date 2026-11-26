//! Folding, indentation guides, the matching bracket and occurrence highlights: what the editor
//! derives from the text's structure. Everything here reads only the visible lines, the
//! folded blocks, or a bounded neighbourhood of the cursor, so a keystroke in a huge file stays
//! cheap; whole-text work happens only on explicit actions (fold all).

use rustext_core::EditedLines;
use rustext_core::decoration::{Decoration, DecorationKind};
use rustext_core::display::{DisplayLayout, DisplayRow, RowContent};
use rustext_core::find::{self, SearchQuery};
use rustext_core::motion::HiddenLines;
use rustext_core::structure::{
    FoldRange, active_indent_guide, fold_range_at, indent_level, indentation_fold_ranges, is_fold_header,
    matching_bracket_pair,
};

use super::CodeEditor;
use super::layout::to_rgba;
use crate::keymap::Action;

/// How far from the cursor the active indent guide's block is searched.
const MAX_ACTIVE_GUIDE_LINES: usize = 5_000;

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
    layout_key: Option<(usize, Vec<FoldRange>, u64)>,
    pub(super) layout: Option<DisplayLayout>,
}

impl FoldState {
    pub fn folded_header_count(&self) -> usize {
        self.blocks.len()
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

impl CodeEditor<'_> {
    /// The folded ranges: untouched blocks as they are, touched ones computed again (and
    /// dropped when their header no longer starts a fold).
    fn folded_ranges(&mut self) -> Vec<FoldRange> {
        let text = self.state.text().clone();
        let indentation = self.state.config.indentation;
        let mut ranges = Vec::with_capacity(self.view.folds.blocks.len());
        self.view.folds.blocks.retain_mut(|block| {
            if !block.needs_recompute {
                ranges.push(FoldRange { header_line: block.header_line, last_hidden_line: block.last_hidden_line });
                return true;
            }
            let range = fold_range_at(&text, &indentation, block.header_line)
                .or_else(|| block.other_header_line.and_then(|line| fold_range_at(&text, &indentation, line)));
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

    /// Whether `line` starts a fold range (a local check, fine for every visible line).
    pub(super) fn is_fold_header(&self, line: usize) -> bool {
        is_fold_header(self.state.text(), &self.state.config.indentation, line)
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
        let indentation = self.state.config.indentation;
        let cursor_line = text.line_of_offset(self.state.selections().primary().head);
        let holds_cursor = |range: &FoldRange| range.header_line == cursor_line || range.hides(cursor_line);
        match action {
            Action::Fold => {
                // The innermost unfolded range holding the cursor: headers looked up from it.
                let header = (0..=cursor_line).rev().find(|&line| {
                    !self.is_folded(line)
                        && fold_range_at(&text, &indentation, line).is_some_and(|range| holds_cursor(&range))
                });
                if let Some(header) = header {
                    self.toggle_fold(header);
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
                let ranges = indentation_fold_ranges(&text, &indentation);
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
        if self.display_layout.is_some() || (!self.config.folding && !self.view.wraps.is_wrapping()) {
            self.view.folds.layout = None;
            self.state.hidden_lines = HiddenLines::default();
            return;
        }
        let cursor_lines: Vec<usize> =
            self.state.selections().iter().map(|selection| text.line_of_offset(selection.head)).collect();
        for range in self.folded_ranges() {
            if cursor_lines.iter().any(|&line| range.hides(line)) {
                self.unfold(range.header_line);
            }
        }
        let folded = if self.config.folding { self.folded_ranges() } else { Vec::new() };
        let folds = &mut self.view.folds;
        if folded.is_empty() && !self.view.wraps.is_wrapping() {
            folds.layout = None;
            folds.layout_key = None;
            self.state.hidden_lines = HiddenLines::default();
            return;
        }
        let key = (text.line_count(), folded, self.view.wraps.generation());
        if folds.layout.is_some() && folds.layout_key.as_ref() == Some(&key) {
            return;
        }
        let folded = key.1.clone();

        let hidden_lines =
            HiddenLines::new(folded.iter().map(|range| (range.first_hidden_line(), range.last_hidden_line)).collect());
        let wraps = &self.view.wraps;
        let rows = hidden_lines
            .shown_lines(text.line_count())
            .flat_map(|line| {
                let continuations = wraps
                    .points(line)
                    .iter()
                    .map(move |&start_column| DisplayRow::new(RowContent::Wrapped { line, start_column }));
                std::iter::once(DisplayRow::new(RowContent::Line(line))).chain(continuations)
            })
            .collect();
        folds.layout = Some(DisplayLayout::new(rows, text.line_count()));
        folds.layout_key = Some(key);
        self.state.hidden_lines = hidden_lines;
    }

    /// Indentation level of each line in `lines`, and the active guide as (level, first line,
    /// last line).
    pub(super) fn indent_guides(
        &self,
        lines: std::ops::Range<usize>,
        cursor_line: usize,
    ) -> (Vec<usize>, Option<(usize, usize, usize)>) {
        let (text, indentation) = (self.state.text(), &self.state.config.indentation);
        let levels = lines.map(|line| indent_level(text, indentation, line)).collect();
        (levels, active_indent_guide(text, indentation, cursor_line, MAX_ACTIVE_GUIDE_LINES))
    }

    /// Borders around the bracket at the primary cursor and its match.
    pub(super) fn bracket_decorations(&mut self) -> Vec<Decoration> {
        if !self.config.match_brackets {
            return Vec::new();
        }
        let head = self.state.selections().primary().head;
        let pair = matching_bracket_pair(
            self.state.text(),
            head,
            &self.state.language,
            &mut self.view.bracket_line_starts,
            &mut self.bracket_budget_lines,
        );
        let Some((open, close)) = pair else {
            return Vec::new();
        };
        let color = to_rgba(self.theme.matching_bracket_border);
        [open, close]
            .into_iter()
            .map(|offset| Decoration::new(offset, offset + 1, DecorationKind::Border(color)))
            .collect()
    }

    /// Backgrounds on the other occurrences of the selected text, or of the word at the cursor,
    /// within the char offsets `visible_start..visible_end`.
    pub(super) fn occurrence_decorations(&self, visible_start: usize, visible_end: usize) -> Vec<Decoration> {
        const MAX_HIGHLIGHTED: usize = 1_000;
        let text = self.state.text();
        let primary = self.state.selections().primary();
        let (query, own_start) = if primary.is_empty() {
            if !self.config.occurrences_highlight {
                return Vec::new();
            }
            let Some((start, end)) = rustext_core::cursors::word_range_at(text, &self.state.config, primary.head)
            else {
                return Vec::new();
            };
            (SearchQuery { match_case: true, whole_word: true, ..SearchQuery::new(text.slice(start, end)) }, start)
        } else {
            let selected = text.slice(primary.start(), primary.end());
            if !self.config.selection_highlight || selected.contains('\n') || selected.trim().is_empty() {
                return Vec::new();
            }
            (SearchQuery { match_case: true, ..SearchQuery::new(selected) }, primary.start())
        };
        let Ok(matches) = find::find_matches(text, &query, Some((visible_start, visible_end)), MAX_HIGHLIGHTED) else {
            return Vec::new();
        };
        if matches.len() < 2 && matches.first().is_some_and(|found| found.start == own_start) {
            return Vec::new();
        }
        let color = to_rgba(self.theme.occurrence_background);
        matches.iter().map(|found| Decoration::new(found.start, found.end, DecorationKind::Background(color))).collect()
    }
}

/// Lines a fold placeholder `⋯` follows: the folded headers, sorted.
pub(super) fn folded_header_lines(folds: &FoldState) -> Vec<usize> {
    folds.blocks.iter().map(|block| block.header_line).collect()
}
