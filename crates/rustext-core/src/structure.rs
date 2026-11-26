//! Structure read from the text alone: matching brackets, indentation guides, indentation
//! folding ranges. Language-aware versions (tree-sitter) can replace them later (D13).

use crate::indentation::Indentation;
use crate::text::Text;

/// Brackets further apart than this are not matched (keeps the search bounded).
const MAX_BRACKET_DISTANCE: usize = 200_000;

/// The bracket touching `offset` (right after it, else right before it) and its match, as two
/// char offsets in text order.
pub fn matching_bracket_pair(text: &Text, offset: usize, brackets: &[(char, char)]) -> Option<(usize, usize)> {
    let at = |position: usize| text.char_at(position);
    let candidates = [Some(offset), offset.checked_sub(1)];
    candidates.into_iter().flatten().find_map(|position| {
        let character = at(position)?;
        let (open, close) = *brackets.iter().find(|(open, close)| *open == character || *close == character)?;
        let matched = if character == open {
            find_closing(text, position, open, close)
        } else {
            find_opening(text, position, open, close)
        }?;
        Some((position.min(matched), position.max(matched)))
    })
}

fn find_closing(text: &Text, open_offset: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (distance, character) in text.chars_from(open_offset + 1).enumerate().take(MAX_BRACKET_DISTANCE) {
        if character == open {
            depth += 1;
        } else if character == close {
            if depth == 0 {
                return Some(open_offset + 1 + distance);
            }
            depth -= 1;
        }
    }
    None
}

fn find_opening(text: &Text, close_offset: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (distance, character) in text.chars_before(close_offset).enumerate().take(MAX_BRACKET_DISTANCE) {
        if character == close {
            depth += 1;
        } else if character == open {
            if depth == 0 {
                return Some(close_offset - 1 - distance);
            }
            depth -= 1;
        }
    }
    None
}

/// Bracket nesting depth at the start of each line, computed lazily and kept across frames:
/// an edit drops the depths after its first line, painting extends them to the painted lines
/// (at most `max_lines_per_call` lines per call, like the highlighter).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BracketDepths {
    /// `depths[line]`: open brackets before `line` (never negative: extra closers are ignored).
    depths: Vec<usize>,
}

impl BracketDepths {
    /// The text changed from `first_changed_line` on.
    pub fn invalidate_from(&mut self, first_changed_line: usize) {
        self.depths.truncate(first_changed_line + 1);
    }

    /// Depth at the start of `line`, `None` if it would take more than `max_lines` lines of
    /// work to know.
    pub fn depth_at(&mut self, text: &Text, brackets: &[(char, char)], line: usize, max_lines: usize) -> Option<usize> {
        if self.depths.is_empty() {
            self.depths.push(0);
        }
        let target = line.min(text.last_line());
        if target + 1 > self.depths.len() + max_lines {
            for _ in 0..max_lines {
                self.extend_one(text, brackets);
            }
            return None;
        }
        while self.depths.len() <= target {
            self.extend_one(text, brackets);
        }
        Some(self.depths[target])
    }

    fn extend_one(&mut self, text: &Text, brackets: &[(char, char)]) {
        let line = self.depths.len() - 1;
        let mut depth = self.depths[line];
        for character in text.chars_from(text.line_start(line)).take_while(|&character| character != '\n') {
            if brackets.iter().any(|(open, _)| *open == character) {
                depth += 1;
            } else if brackets.iter().any(|(_, close)| *close == character) {
                depth = depth.saturating_sub(1);
            }
        }
        self.depths.push(depth);
    }
}

/// Columns of the brackets of `line` with their nesting depth (the depth inside an opening
/// bracket's pair, which its closing bracket shares).
pub fn bracket_columns(
    line_content: &str,
    depth_at_line_start: usize,
    brackets: &[(char, char)],
) -> Vec<(usize, usize)> {
    let mut depth = depth_at_line_start;
    let mut columns = Vec::new();
    for (column, character) in line_content.chars().enumerate() {
        if brackets.iter().any(|(open, _)| *open == character) {
            columns.push((column, depth));
            depth += 1;
        } else if brackets.iter().any(|(_, close)| *close == character) {
            depth = depth.saturating_sub(1);
            columns.push((column, depth));
        }
    }
    columns
}

/// Visual width of the indentation of `line`, `None` for a blank line. Reads only the
/// line's leading whitespace, without allocating.
pub fn line_indent(text: &Text, indentation: &Indentation, line: usize) -> Option<usize> {
    let tab_size = indentation.tab_size.max(1);
    let mut width = 0;
    for character in text.chars_from(text.line_start(line)) {
        match character {
            ' ' => width += 1,
            '\t' => width = (width / tab_size + 1) * tab_size,
            '\n' | '\r' => return None,
            _ => return Some(width),
        }
    }
    None
}

/// Lines searched at most for the nearest non-blank line (keeps blank runs cheap).
const MAX_BLANK_SEARCH: usize = 2_000;

fn nearest_indent(text: &Text, indentation: &Indentation, lines: impl Iterator<Item = usize>) -> usize {
    lines.take(MAX_BLANK_SEARCH).find_map(|line| line_indent(text, indentation, line)).unwrap_or(0)
}

/// Indentation level of a line, in indent units; a blank line takes the smaller level of the
/// nearest non-blank lines around it (so guides run through blank lines inside blocks).
pub fn indent_level(text: &Text, indentation: &Indentation, line: usize) -> usize {
    let tab_size = indentation.tab_size.max(1);
    let width = line_indent(text, indentation, line).unwrap_or_else(|| {
        let above = nearest_indent(text, indentation, (0..line).rev());
        let below = nearest_indent(text, indentation, line + 1..text.line_count());
        above.min(below)
    });
    width / tab_size
}

/// Indentation levels of `lines` (the visible ones).
pub fn indent_levels(text: &Text, indentation: &Indentation, lines: std::ops::Range<usize>) -> Vec<usize> {
    lines.map(|line| indent_level(text, indentation, line)).collect()
}

/// The guide to highlight for a cursor on `cursor_line`, as (level, first line, last line),
/// searching at most `max_lines` away from the cursor. `None` at the top level.
pub fn active_indent_guide(
    text: &Text,
    indentation: &Indentation,
    cursor_line: usize,
    max_lines: usize,
) -> Option<(usize, usize, usize)> {
    let block_level = indent_level(text, indentation, cursor_line);
    if block_level == 0 {
        return None;
    }
    let is_in_block = |line: usize| indent_level(text, indentation, line) >= block_level;
    let first_line = (cursor_line.saturating_sub(max_lines)..cursor_line)
        .rev()
        .take_while(|&line| is_in_block(line))
        .last()
        .unwrap_or(cursor_line);
    let last_line = (cursor_line + 1..text.line_count().min(cursor_line + max_lines))
        .take_while(|&line| is_in_block(line))
        .last()
        .unwrap_or(cursor_line);
    Some((block_level - 1, first_line, last_line))
}

/// A foldable region: `header_line` stays visible, `first_hidden_line..=last_hidden_line` hide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FoldRange {
    pub header_line: usize,
    pub last_hidden_line: usize,
}

impl FoldRange {
    pub fn first_hidden_line(&self) -> usize {
        self.header_line + 1
    }

    pub fn hides(&self, line: usize) -> bool {
        (self.first_hidden_line()..=self.last_hidden_line).contains(&line)
    }
}

/// The next non-blank line after `line` with its indentation.
fn next_non_blank(text: &Text, indentation: &Indentation, line: usize) -> Option<(usize, usize)> {
    (line + 1..text.line_count())
        .take(MAX_BLANK_SEARCH)
        .find_map(|next| line_indent(text, indentation, next).map(|indent| (next, indent)))
}

/// Whether `line` starts an indentation fold: the next non-blank line is more indented. Local:
/// cheap enough for every visible line.
pub fn is_fold_header(text: &Text, indentation: &Indentation, line: usize) -> bool {
    let Some(indent) = line_indent(text, indentation, line) else { return false };
    next_non_blank(text, indentation, line).is_some_and(|(_, next_indent)| next_indent > indent)
}

/// The indentation fold starting at `header_line` (Monaco's `foldingStrategy: 'indentation'`):
/// the more indented lines after it, up to the last of them that is not blank. Reads the
/// whole block.
pub fn fold_range_at(text: &Text, indentation: &Indentation, header_line: usize) -> Option<FoldRange> {
    if !is_fold_header(text, indentation, header_line) {
        return None;
    }
    let header_indent = line_indent(text, indentation, header_line)?;
    let mut last_hidden_line = header_line;
    for line in header_line + 1..text.line_count() {
        match line_indent(text, indentation, line) {
            Some(indent) if indent <= header_indent => break,
            Some(_) => last_hidden_line = line,
            None => {}
        }
    }
    Some(FoldRange { header_line, last_hidden_line })
}

/// The innermost fold whose header is `line` or that hides `line`: looks up for headers less
/// indented than the line.
pub fn enclosing_fold_range(text: &Text, indentation: &Indentation, line: usize) -> Option<FoldRange> {
    if let Some(range) = fold_range_at(text, indentation, line) {
        return Some(range);
    }
    let mut indent =
        line_indent(text, indentation, line).unwrap_or_else(|| nearest_indent(text, indentation, (0..line).rev()));
    for candidate in (0..line).rev() {
        let Some(candidate_indent) = line_indent(text, indentation, candidate) else { continue };
        if candidate_indent >= indent {
            continue;
        }
        if let Some(range) = fold_range_at(text, indentation, candidate).filter(|range| range.hides(line)) {
            return Some(range);
        }
        indent = candidate_indent;
        if indent == 0 {
            break;
        }
    }
    None
}

/// Every indentation fold of the text, sorted by header line (for "fold all").
pub fn indentation_fold_ranges(text: &Text, indentation: &Indentation) -> Vec<FoldRange> {
    let mut ranges = Vec::new();
    // Headers whose block is still open, with their indentation width.
    let mut open_headers: Vec<(usize, usize)> = Vec::new();
    let mut last_non_blank_line = 0;
    for line in 0..text.line_count() {
        let Some(width) = line_indent(text, indentation, line) else { continue };
        while let Some(&(header_line, header_width)) = open_headers.last() {
            if width > header_width {
                break;
            }
            open_headers.pop();
            if last_non_blank_line > header_line {
                ranges.push(FoldRange { header_line, last_hidden_line: last_non_blank_line });
            }
        }
        open_headers.push((line, width));
        last_non_blank_line = line;
    }
    while let Some((header_line, _)) = open_headers.pop() {
        if last_non_blank_line > header_line {
            ranges.push(FoldRange { header_line, last_hidden_line: last_non_blank_line });
        }
    }
    ranges.sort_by_key(|range| range.header_line);
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    const BRACKETS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}')];

    #[test]
    fn matches_brackets_after_then_before_the_cursor() {
        let text = Text::new("f(a[1], (b))");

        assert_eq!(matching_bracket_pair(&text, 1, BRACKETS), Some((1, 11)));
        assert_eq!(matching_bracket_pair(&text, 12, BRACKETS), Some((1, 11)));
        assert_eq!(matching_bracket_pair(&text, 4, BRACKETS), Some((3, 5)));
        assert_eq!(matching_bracket_pair(&text, 2, BRACKETS), Some((1, 11)));
        assert_eq!(matching_bracket_pair(&Text::new("ab"), 1, BRACKETS), None);
    }

    #[test]
    fn bracket_depths_follow_nesting_across_lines() {
        let text = Text::new("f(a, {\n  [b]\n})");
        let mut depths = BracketDepths::default();

        assert_eq!(depths.depth_at(&text, BRACKETS, 1, 100), Some(2));
        assert_eq!(bracket_columns("  [b]", 2, BRACKETS), vec![(2, 2), (4, 2)]);
        assert_eq!(depths.depth_at(&text, BRACKETS, 2, 100), Some(2));
        assert_eq!(bracket_columns("})", 2, BRACKETS), vec![(0, 1), (1, 0)]);
    }

    #[test]
    fn bracket_depths_have_a_budget_and_recompute_after_edits() {
        let text = Text::new(&"(\n".repeat(50));
        let mut depths = BracketDepths::default();

        assert_eq!(depths.depth_at(&text, BRACKETS, 40, 10), None);
        assert_eq!(depths.depth_at(&text, BRACKETS, 40, 100), Some(40));
        depths.invalidate_from(5);
        let edited = Text::new(&")\n".repeat(50));
        assert_eq!(depths.depth_at(&edited, BRACKETS, 7, 100), Some(3));
    }

    #[test]
    fn unmatched_bracket_has_no_pair() {
        assert_eq!(matching_bracket_pair(&Text::new("(("), 0, BRACKETS), None);
    }

    #[test]
    fn blank_lines_take_the_smaller_neighbour_level() {
        let text = Text::new("a\n    b\n\n    c\n\nd");

        assert_eq!(indent_levels(&text, &Indentation::default(), 0..6), vec![0, 1, 1, 1, 0, 0]);
    }

    #[test]
    fn active_guide_spans_the_cursor_block() {
        let text = Text::new("a\n    b\n        c\n    d\n    e\nf");
        let indentation = Indentation::default();

        assert_eq!(active_indent_guide(&text, &indentation, 3, 100), Some((0, 1, 4)));
        assert_eq!(active_indent_guide(&text, &indentation, 2, 100), Some((1, 2, 2)));
        assert_eq!(active_indent_guide(&text, &indentation, 0, 100), None);
    }

    #[test]
    fn local_fold_queries_agree_with_the_whole_text_ranges() {
        let text = Text::new("fn a() {\n    if x {\n        y();\n    }\n\n    z();\n}\nfn b() {}");
        let indentation = Indentation::default();

        for range in indentation_fold_ranges(&text, &indentation) {
            assert!(is_fold_header(&text, &indentation, range.header_line));
            assert_eq!(fold_range_at(&text, &indentation, range.header_line), Some(range));
        }
        assert_eq!(
            enclosing_fold_range(&text, &indentation, 2),
            Some(FoldRange { header_line: 1, last_hidden_line: 2 })
        );
        assert_eq!(
            enclosing_fold_range(&text, &indentation, 5),
            Some(FoldRange { header_line: 0, last_hidden_line: 5 })
        );
        assert_eq!(
            enclosing_fold_range(&text, &indentation, 4),
            Some(FoldRange { header_line: 0, last_hidden_line: 5 })
        );
        assert_eq!(enclosing_fold_range(&text, &indentation, 7), None);
    }

    #[test]
    fn line_indent_expands_tabs_and_skips_blank_lines() {
        let text = Text::new("\t  x\n   \ny");
        let indentation = Indentation { tab_size: 4, insert_spaces: false };

        assert_eq!(line_indent(&text, &indentation, 0), Some(6));
        assert_eq!(line_indent(&text, &indentation, 1), None);
        assert_eq!(line_indent(&text, &indentation, 2), Some(0));
    }

    #[test]
    fn folds_indented_blocks_keeping_the_closing_line() {
        let text = Text::new("fn a() {\n    if x {\n        y();\n    }\n\n    z();\n}\nfn b() {}");

        let ranges = indentation_fold_ranges(&text, &Indentation::default());

        assert_eq!(
            ranges,
            vec![FoldRange { header_line: 0, last_hidden_line: 5 }, FoldRange { header_line: 1, last_hidden_line: 2 }]
        );
    }

    #[test]
    fn trailing_blank_lines_are_not_folded() {
        let text = Text::new("a\n  b\n\n\nc");

        assert_eq!(
            indentation_fold_ranges(&text, &Indentation::default()),
            vec![FoldRange { header_line: 0, last_hidden_line: 1 }]
        );
    }
}
