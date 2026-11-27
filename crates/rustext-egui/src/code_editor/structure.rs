//! Folding, indentation guides, the matching bracket and occurrence highlights: what the editor
//! derives from the text's structure. Everything here reads only the visible lines, the
//! folded blocks, or a bounded neighbourhood of the cursor, so a keystroke in a huge file stays
//! cheap; whole-text work happens only on explicit actions (fold all).

use rustext_core::EditedLines;
use rustext_core::decoration::{Decoration, DecorationKind};
use rustext_core::display::{DisplayLayout, DisplayRow, RowContent};
use rustext_core::find::{self, SearchQuery};
use rustext_core::indentation::Indentation;
use rustext_core::motion::HiddenLines;
use rustext_core::structure::{
    BracketScanner, EnclosingBracketPair, FoldRange, active_indent_guide, enclosing_bracket_pair, fold_range_at,
    indent_level, indentation_fold_ranges, matching_bracket_pair,
};
use rustext_core::style::LineStyler;
use rustext_core::{Selections, Text};

use super::layout::to_rgba;
use super::{CodeEditor, FoldingStrategy};
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
    /// What the layout was built from: line count, folded ranges, wrap generation, code lens
    /// lines.
    layout_key: Option<(usize, Vec<FoldRange>, u64, Vec<usize>)>,
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

/// The fold range `line` starts: from the styler's syntax tree when it has one and the
/// strategy allows, else from indentation.
fn fold_range_of<'styler>(
    styler: Option<&mut (dyn LineStyler + 'styler)>,
    strategy: FoldingStrategy,
    text: &Text,
    indentation: &Indentation,
    line: usize,
) -> Option<FoldRange> {
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
                let ranges: Vec<FoldRange> = if self.folds_by_syntax() {
                    (0..text.line_count()).filter_map(|line| self.fold_range_at_line(line)).collect()
                } else {
                    indentation_fold_ranges(&text, &self.state.config.indentation)
                };
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
        if self.display_layout.is_some() || !needs_layout {
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

    /// The bracket pair holding the primary cursor, kept until the text or the cursor changes;
    /// asked again next frame when the bracket line states ran out of this frame's budget.
    fn active_bracket_pair(&mut self) -> Option<EnclosingBracketPair> {
        let head = self.state.selections().primary().head;
        let key = (self.state.text_revision(), head);
        if let Some((cached_key, pair)) = self.view.active_bracket_pair
            && cached_key == key
        {
            return pair;
        }
        let mut scanner = BracketScanner::new(&self.state.language, self.styler.as_deref_mut());
        let line_starts = &mut self.view.bracket_line_starts;
        let pair =
            enclosing_bracket_pair(self.state.text(), head, &mut scanner, line_starts, &mut self.bracket_budget_lines);
        if pair.is_some() || self.bracket_budget_lines > 0 {
            self.view.active_bracket_pair = Some((key, pair));
        }
        pair
    }

    /// The bracket pair guide: a vertical line in the pair's color, at the leftmost of its
    /// brackets' columns, along the lines between them.
    pub(super) fn paint_bracket_pair_guide(&mut self, painter: &egui::Painter, metrics: &super::Metrics) {
        if !self.config.bracket_pair_guides {
            return;
        }
        let Some(pair) = self.active_bracket_pair() else { return };
        let text = self.state.text();
        let (open, close) = (text.position(pair.open), text.position(pair.close));
        if close.line <= open.line {
            return;
        }
        let indentation = &self.state.config.indentation;
        let visual_column = |position: rustext_core::Position| {
            indentation.visual_column(&text.line_content(position.line), position.column)
        };
        let column = visual_column(open).min(visual_column(close));
        let x = (self.text_left(metrics) + column as f32 * metrics.char_width).round() + 0.5;
        let top = self.row_top(metrics, self.row_of_line(open.line)) + metrics.row_height;
        let bottom = self.row_top(metrics, self.row_of_line(close.line));
        let colors = &self.theme.bracket_pair_colors;
        let color = if colors.is_empty() { self.theme.active_indent_guide } else { colors[pair.depth % colors.len()] };
        painter.vline(x, top..=bottom, egui::Stroke::new(1.0, color));
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
            &mut BracketScanner::new(&self.state.language, self.styler.as_deref_mut()),
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
    /// The URL at `offset`, when links are on.
    pub(super) fn link_at(&self, offset: usize) -> Option<String> {
        if !self.config.links {
            return None;
        }
        let text = self.state.text();
        let position = text.position(offset);
        let links = rustext_core::links::links_in_line(&text.line_content(position.line));
        links
            .into_iter()
            .find(|link| (link.start_column..link.end_column).contains(&position.column))
            .map(|link| link.url)
    }

    /// Underlines under the links of `lines`: subtle, in the link color under the pointer when
    /// Ctrl is held (a click opens it then).
    pub(super) fn link_decorations(
        &self,
        lines: std::ops::RangeInclusive<usize>,
        ctrl_hovered: Option<usize>,
    ) -> Vec<Decoration> {
        if !self.config.links {
            return Vec::new();
        }
        let text = self.state.text();
        let mut decorations = Vec::new();
        for line in lines.filter(|&line| line < text.line_count()) {
            let line_start = text.line_start(line);
            for link in rustext_core::links::links_in_line(&text.line_content(line)) {
                let (start, end) = (line_start + link.start_column, line_start + link.end_column);
                let is_active = ctrl_hovered.is_some_and(|offset| (start..end).contains(&offset));
                let color = if is_active { self.theme.link } else { self.theme.line_number };
                decorations.push(Decoration::new(start, end, DecorationKind::Underline(to_rgba(color))));
            }
        }
        decorations
    }

    pub(super) fn occurrence_decorations(&self, visible_start: usize, visible_end: usize) -> Vec<Decoration> {
        const MAX_HIGHLIGHTED: usize = 1_000;
        let text = self.state.text();
        let primary = self.state.selections().primary();
        let (query, own_start) = if primary.is_empty() {
            if !self.config.occurrences_highlight {
                return Vec::new();
            }
            let service_ranges = self.view.document_highlights.ranges_for(self.state.text_revision(), primary.head);
            if let Some(ranges) = service_ranges {
                let color = to_rgba(self.theme.occurrence_background);
                return ranges
                    .iter()
                    .filter(|&&(start, end)| end >= visible_start && start <= visible_end)
                    .map(|&(start, end)| Decoration::new(start, end, DecorationKind::Background(color)))
                    .collect();
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
