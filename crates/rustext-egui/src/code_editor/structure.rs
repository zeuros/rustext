//! Folding, indentation guides, the matching bracket and occurrence highlights: what the editor
//! derives from the text's structure each frame, cached by text revision.

use rustext_core::Text;
use rustext_core::change::{Bias, ChangeSet};
use rustext_core::decoration::{Decoration, DecorationKind};
use rustext_core::display::{DisplayLayout, DisplayRow, RowContent};
use rustext_core::find::{self, SearchQuery};
use rustext_core::motion::HiddenLines;
use rustext_core::structure::{FoldRange, indent_levels, indentation_fold_ranges, matching_bracket_pair};

use super::CodeEditor;
use super::layout::to_rgba;
use crate::keymap::Action;

/// Per-view folding state and the caches derived from the text.
#[derive(Clone, Debug, Default)]
pub struct FoldState {
    /// Char offsets of the starts of the folded header lines; they follow edits.
    folded_header_offsets: Vec<usize>,
    /// Fold ranges and indent levels of the text revision (and tab size) they were computed for.
    ranges: Option<(u64, usize, Vec<FoldRange>)>,
    indent_levels: Option<(u64, usize, Vec<usize>)>,
    /// The rows when something is folded.
    pub(super) layout: Option<DisplayLayout>,
}

impl FoldState {
    pub fn folded_header_count(&self) -> usize {
        self.folded_header_offsets.len()
    }

    /// Folds the region whose header line starts at char offset `header_line_start` (kept only
    /// if a fold range starts there).
    pub fn fold_at(&mut self, header_line_start: usize) {
        self.folded_header_offsets.push(header_line_start);
    }

    /// Folded header offsets after the text changed.
    pub(super) fn map_through(&mut self, changes: &ChangeSet) {
        for offset in &mut self.folded_header_offsets {
            *offset = changes.map_offset(*offset, Bias::Before);
        }
    }
}

impl CodeEditor<'_> {
    pub(super) fn fold_ranges(&mut self) -> Vec<FoldRange> {
        let revision = self.state.text_revision();
        let tab_size = self.state.config.indentation.tab_size;
        let folds = &mut self.view.folds;
        match &folds.ranges {
            Some((cached_revision, cached_tab_size, ranges))
                if *cached_revision == revision && *cached_tab_size == tab_size =>
            {
                ranges.clone()
            }
            _ => {
                let ranges = indentation_fold_ranges(self.state.text(), &self.state.config.indentation);
                folds.ranges = Some((revision, tab_size, ranges.clone()));
                ranges
            }
        }
    }

    pub(super) fn indent_levels(&mut self) -> Vec<usize> {
        let revision = self.state.text_revision();
        let tab_size = self.state.config.indentation.tab_size;
        let folds = &mut self.view.folds;
        match &folds.indent_levels {
            Some((cached_revision, cached_tab_size, levels))
                if *cached_revision == revision && *cached_tab_size == tab_size =>
            {
                levels.clone()
            }
            _ => {
                let levels = indent_levels(self.state.text(), &self.state.config.indentation);
                folds.indent_levels = Some((revision, tab_size, levels.clone()));
                levels
            }
        }
    }

    /// The fold ranges currently folded.
    fn folded_ranges(&mut self) -> Vec<FoldRange> {
        let text = self.state.text().clone();
        let folded_lines: Vec<usize> =
            self.view.folds.folded_header_offsets.iter().map(|&offset| text.line_of_offset(offset)).collect();
        self.fold_ranges().into_iter().filter(|range| folded_lines.contains(&range.header_line)).collect()
    }

    /// The fold range whose header is `line`.
    pub(super) fn fold_range_at(&mut self, line: usize) -> Option<FoldRange> {
        self.fold_ranges().into_iter().find(|range| range.header_line == line)
    }

    pub(super) fn is_folded(&self, header_line: usize) -> bool {
        let text = self.state.text();
        self.view.folds.folded_header_offsets.iter().any(|&offset| text.line_of_offset(offset) == header_line)
    }

    pub(super) fn toggle_fold(&mut self, header_line: usize) {
        if self.is_folded(header_line) {
            self.unfold(header_line);
        } else if self.fold_range_at(header_line).is_some() {
            let offset = self.state.text().line_start(header_line);
            self.view.folds.folded_header_offsets.push(offset);
        }
    }

    fn unfold(&mut self, header_line: usize) {
        let text = self.state.text().clone();
        self.view.folds.folded_header_offsets.retain(|&offset| text.line_of_offset(offset) != header_line);
    }

    /// Runs the folding actions; returns the actions it doesn't handle.
    pub(super) fn run_fold_action(&mut self, action: Action) -> Option<Action> {
        let cursor_line = self.state.text().line_of_offset(self.state.selections().primary().head);
        let innermost_range_at_cursor = |ranges: &[FoldRange]| {
            ranges
                .iter()
                .filter(|range| range.header_line == cursor_line || range.hides(cursor_line))
                .max_by_key(|range| range.header_line)
                .copied()
        };
        match action {
            Action::Fold => {
                let ranges = self.fold_ranges();
                let folded: Vec<FoldRange> = self.folded_ranges();
                let unfolded: Vec<FoldRange> = ranges.into_iter().filter(|range| !folded.contains(range)).collect();
                if let Some(range) = innermost_range_at_cursor(&unfolded) {
                    self.toggle_fold(range.header_line);
                }
                None
            }
            Action::Unfold => {
                if let Some(range) = innermost_range_at_cursor(&self.folded_ranges()) {
                    self.unfold(range.header_line);
                }
                None
            }
            Action::FoldAll => {
                let text = self.state.text().clone();
                self.view.folds.folded_header_offsets =
                    self.fold_ranges().iter().map(|range| text.line_start(range.header_line)).collect();
                None
            }
            Action::UnfoldAll => {
                self.view.folds.folded_header_offsets.clear();
                None
            }
            other => Some(other),
        }
    }

    /// Rebuilds the folded rows: unfolds what hides a cursor, then hides the folded lines.
    pub(super) fn refresh_folding(&mut self) {
        if !self.config.folding || self.display_layout.is_some() {
            self.view.folds.layout = None;
            self.state.hidden_lines = HiddenLines::default();
            return;
        }
        let text = self.state.text().clone();
        let cursor_lines: Vec<usize> =
            self.state.selections().iter().map(|selection| text.line_of_offset(selection.head)).collect();
        for range in self.folded_ranges() {
            if cursor_lines.iter().any(|&line| range.hides(line)) {
                self.unfold(range.header_line);
            }
        }
        let folded = self.folded_ranges();
        self.view
            .folds
            .folded_header_offsets
            .retain(|&offset| folded.iter().any(|range| text.line_start(range.header_line) == offset));
        self.view.folds.folded_header_offsets.dedup();
        if folded.is_empty() {
            self.view.folds.layout = None;
            self.state.hidden_lines = HiddenLines::default();
            return;
        }

        let hidden_lines =
            HiddenLines::new(folded.iter().map(|range| (range.first_hidden_line(), range.last_hidden_line)).collect());
        let rows = (0..text.line_count())
            .filter(|&line| !hidden_lines.is_hidden(line))
            .map(|line| DisplayRow::new(RowContent::Line(line)))
            .collect();
        self.view.folds.layout = Some(DisplayLayout::new(rows, text.line_count()));
        self.state.hidden_lines = hidden_lines;
    }

    /// Borders around the bracket at the primary cursor and its match.
    pub(super) fn bracket_decorations(&self) -> Vec<Decoration> {
        if !self.config.match_brackets {
            return Vec::new();
        }
        let head = self.state.selections().primary().head;
        let Some((open, close)) = matching_bracket_pair(self.state.text(), head, &self.state.language.brackets) else {
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
        const MAX_HIGHLIGHTED: usize = 1_000;
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

/// Lines a fold placeholder `⋯` follows: the folded headers.
pub(super) fn folded_header_lines(text: &Text, folds: &FoldState) -> Vec<usize> {
    folds.folded_header_offsets.iter().map(|&offset| text.line_of_offset(offset)).collect()
}
