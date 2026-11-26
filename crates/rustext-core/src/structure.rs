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

/// Indentation level of every line, in indent units; blank lines take the smaller level of
/// the nearest non-blank lines around them (so guides run through blank lines inside blocks).
pub fn indent_levels(text: &Text, indentation: &Indentation) -> Vec<usize> {
    let tab_size = indentation.tab_size.max(1);
    let own_levels: Vec<Option<usize>> = text
        .lines()
        .map(|content| (!content.trim().is_empty()).then(|| indentation.indent_width(&content) / tab_size))
        .collect();

    let mut levels = vec![0; own_levels.len()];
    let mut previous_level = 0;
    for (line, level) in own_levels.iter().enumerate() {
        if let Some(level) = level {
            previous_level = *level;
        }
        levels[line] = level.unwrap_or(previous_level);
    }
    let mut next_level = 0;
    for (line, level) in own_levels.iter().enumerate().rev() {
        match level {
            Some(level) => next_level = *level,
            None => levels[line] = levels[line].min(next_level),
        }
    }
    levels
}

/// The guide to highlight for a cursor on `cursor_line`: its level (guide index, 0-based) and
/// the lines it spans. `None` at the top level.
pub fn active_indent_guide(levels: &[usize], cursor_line: usize) -> Option<(usize, usize, usize)> {
    let cursor_level = *levels.get(cursor_line)?;
    let block_level = if cursor_level == 0 { return None } else { cursor_level };
    let guide = block_level - 1;
    let mut first_line = cursor_line;
    while first_line > 0 && levels[first_line - 1] >= block_level {
        first_line -= 1;
    }
    let mut last_line = cursor_line;
    while last_line + 1 < levels.len() && levels[last_line + 1] >= block_level {
        last_line += 1;
    }
    Some((guide, first_line, last_line))
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

/// Indentation folding (Monaco's `foldingStrategy: 'indentation'`): a non-blank line followed
/// by more indented lines folds them, up to the last of them that is not blank. Sorted by
/// header line.
pub fn indentation_fold_ranges(text: &Text, indentation: &Indentation) -> Vec<FoldRange> {
    let widths: Vec<Option<usize>> =
        text.lines().map(|content| (!content.trim().is_empty()).then(|| indentation.indent_width(&content))).collect();

    let mut ranges = Vec::new();
    // Headers whose block is still open, with their indentation width.
    let mut open_headers: Vec<(usize, usize)> = Vec::new();
    let mut last_non_blank_line = 0;
    for (line, width) in widths.iter().enumerate() {
        let Some(width) = *width else { continue };
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
    fn unmatched_bracket_has_no_pair() {
        assert_eq!(matching_bracket_pair(&Text::new("(("), 0, BRACKETS), None);
    }

    #[test]
    fn blank_lines_take_the_smaller_neighbour_level() {
        let text = Text::new("a\n    b\n\n    c\n\nd");

        assert_eq!(indent_levels(&text, &Indentation::default()), vec![0, 1, 1, 1, 0, 0]);
    }

    #[test]
    fn active_guide_spans_the_cursor_block() {
        let levels = [0, 1, 2, 1, 1, 0];

        assert_eq!(active_indent_guide(&levels, 3), Some((0, 1, 4)));
        assert_eq!(active_indent_guide(&levels, 2), Some((1, 2, 2)));
        assert_eq!(active_indent_guide(&levels, 0), None);
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
