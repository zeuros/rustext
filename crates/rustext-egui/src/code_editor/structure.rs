//! Indentation guides, the matching bracket, links and occurrence highlights: what the editor
//! derives from the text's structure. Everything here reads only the visible lines or a
//! bounded neighbourhood of the cursor, so a keystroke in a huge file stays cheap.

use rustext_core::decoration::{Decoration, DecorationKind};
use rustext_core::find::{self, SearchQuery};
use rustext_core::structure::{
    BracketScanner, EnclosingBracketPair, active_indent_guide, enclosing_bracket_pair, indent_level,
    matching_bracket_pair,
};

use super::CodeEditor;
use super::layout::to_rgba;

/// How far from the cursor the active indent guide's block is searched.
const MAX_ACTIVE_GUIDE_LINES: usize = 5_000;

impl CodeEditor<'_> {
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
