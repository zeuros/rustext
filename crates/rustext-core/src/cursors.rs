//! Commands that only change the selections: multi-cursor, select word / line / all, column
//! selection. Pure functions from the current selections to the next ones.

use crate::config::EditingConfig;
use crate::edit::covered_lines;
use crate::motion::{Motion, apply_motion};
use crate::position::Position;
use crate::selection::{Selection, Selections};
use crate::text::Text;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VerticalDirection {
    Up,
    Down,
}

/// A new cursor above (or below) every cursor, at the same visual column (Ctrl+Alt+Up/Down).
pub fn add_cursors_vertically(
    text: &Text,
    config: &EditingConfig,
    selections: &Selections,
    direction: VerticalDirection,
) -> Selections {
    let motion = match direction {
        VerticalDirection::Up => Motion::Up,
        VerticalDirection::Down => Motion::Down,
    };
    let edge_line = |selection: &Selection| text.line_of_offset(selection.head);
    let target = match direction {
        VerticalDirection::Up => selections.iter().min_by_key(|selection| edge_line(selection)),
        VerticalDirection::Down => selections.iter().max_by_key(|selection| edge_line(selection)),
    };
    let Some(target) = target.copied() else {
        return selections.clone();
    };
    let is_at_document_edge = match direction {
        VerticalDirection::Up => edge_line(&target) == 0,
        VerticalDirection::Down => edge_line(&target) == text.last_line(),
    };
    if is_at_document_edge || selections.len() >= config.multi_cursor_limit {
        return selections.clone();
    }
    let added = apply_motion(
        text,
        config,
        &Selection::cursor(target.head).with_preferred_visual_column(target.preferred_visual_column),
        motion,
        false,
    );
    selections.added(added)
}

/// Each empty selection grows to the word around it; non-empty ones stay.
pub fn select_words(text: &Text, config: &EditingConfig, selections: &Selections) -> Selections {
    selections.map_each(|selection| {
        if !selection.is_empty() {
            return *selection;
        }
        word_range_at(text, config, selection.head).map_or(*selection, |(start, end)| Selection::new(start, end))
    })
}

/// Offsets of the word touching `offset`.
pub fn word_range_at(text: &Text, config: &EditingConfig, offset: usize) -> Option<(usize, usize)> {
    let position = text.position(offset);
    let chars: Vec<char> = text.line_content(position.line).chars().collect();
    let line_start = text.line_start(position.line);
    let (start_column, end_column) = config.words.word_at(&chars, position.column)?;
    Some((line_start + start_column, line_start + end_column))
}

/// Ctrl+D: the first press selects the word under each empty cursor; the next ones add the next
/// occurrence of the primary selection's text after the last selection, wrapping around.
pub fn add_next_occurrence(text: &Text, config: &EditingConfig, selections: &Selections) -> Selections {
    let primary = selections.primary();
    if primary.is_empty() {
        return select_words(text, config, selections);
    }
    let needle = text.slice(primary.start(), primary.end());
    let content = text.to_string();
    let last_end = selections.iter().map(Selection::end).max().unwrap_or(0);
    let occurrences = selectable_occurrences(text, &content, &needle);
    let is_selected = |start: usize| selections.iter().any(|selection| selection.start() == start);
    let next = occurrences
        .iter()
        .filter(|&&start| start >= last_end)
        .chain(occurrences.iter())
        .find(|&&start| !is_selected(start));
    match next {
        Some(&start) => selections.added(Selection::new(start, start + needle.chars().count())),
        None => selections.clone(),
    }
}

/// Ctrl+Shift+L: every occurrence of the primary selection's text (or of the word under the
/// cursor) becomes a selection.
pub fn select_all_occurrences(text: &Text, config: &EditingConfig, selections: &Selections) -> Selections {
    let primary = selections.primary();
    let (start, end) = if primary.is_empty() {
        match word_range_at(text, config, primary.head) {
            Some(range) => range,
            None => return selections.clone(),
        }
    } else {
        (primary.start(), primary.end())
    };
    let needle = text.slice(start, end);
    let needle_len = end - start;
    let ranges: Vec<Selection> = selectable_occurrences(text, &text.to_string(), &needle)
        .into_iter()
        .take(config.multi_cursor_limit)
        .map(|occurrence_start| Selection::new(occurrence_start, occurrence_start + needle_len))
        .collect();
    let primary_index = ranges.iter().position(|selection| selection.start() == start).unwrap_or(0);
    if ranges.is_empty() { selections.clone() } else { Selections::new(ranges, primary_index) }
}

/// Offsets of the occurrences of `needle` that can be selected: neither end inside a `\r\n`
/// (a `\n` needle also matches the second half of every CRLF).
fn selectable_occurrences(text: &Text, content: &str, needle: &str) -> Vec<usize> {
    let needle_len = needle.chars().count();
    occurrence_offsets(content, needle)
        .into_iter()
        .filter(|&start| {
            text.clamp_offset(start) == start && text.clamp_offset(start + needle_len) == start + needle_len
        })
        .collect()
}

/// Char offsets where `needle` occurs in `haystack`, without overlaps.
pub fn occurrence_offsets(haystack: &str, needle: &str) -> Vec<usize> {
    if needle.is_empty() {
        return Vec::new();
    }
    let mut offsets = Vec::new();
    let mut char_offset = 0;
    let mut previous_byte_offset = 0;
    for (byte_offset, _) in haystack.match_indices(needle) {
        char_offset += haystack[previous_byte_offset..byte_offset].chars().count();
        offsets.push(char_offset);
        previous_byte_offset = byte_offset;
    }
    offsets
}

/// Shift+Alt+I: a cursor at the end of every line each selection covers.
pub fn cursors_at_line_ends(text: &Text, selections: &Selections) -> Selections {
    let cursors: Vec<Selection> = covered_lines(text, selections)
        .into_iter()
        .map(|line| Selection::cursor(text.line_content_end(line)))
        .collect();
    let primary_index = cursors.len() - 1;
    Selections::new(cursors, primary_index)
}

/// Ctrl+L: each selection grows to whole lines, line ending included; pressed again it adds
/// the next line.
pub fn expand_to_lines(text: &Text, selections: &Selections) -> Selections {
    selections.map_each(|selection| {
        let first_line = text.line_of_offset(selection.start());
        let end_position = text.position(selection.end());
        let is_already_whole_lines =
            !selection.is_empty() && end_position.column == 0 && selection.start() == text.line_start(first_line);
        let last_line = if is_already_whole_lines { end_position.line } else { text.line_of_offset(selection.end()) };
        let end = if last_line < text.last_line() { text.line_start(last_line + 1) } else { text.len_chars() };
        Selection::new(text.line_start(first_line), end)
    })
}

pub fn select_all(text: &Text) -> Selections {
    Selections::single(Selection::new(0, text.len_chars()))
}

/// Escape: several selections become the primary one; a single selection collapses to its head.
pub fn collapse(selections: &Selections) -> Selections {
    if selections.is_multiple() {
        return selections.only_primary();
    }
    Selections::cursor(selections.primary().head)
}

/// Box selection from `anchor` to `head` (Shift+Alt+drag): one selection per line between
/// them, from the anchor's visual column to the head's. Lines too short get a cursor at their end
/// only when the head's column is within them, as in VS Code.
pub fn column_selection(
    text: &Text,
    config: &EditingConfig,
    anchor: Position,
    head_line: usize,
    head_visual_column: usize,
) -> Selections {
    let indentation = config.indentation;
    let anchor_visual_column = indentation.visual_column(&text.line_content(anchor.line), anchor.column);
    let head_line = head_line.min(text.last_line());
    let lines: Vec<usize> = if anchor.line <= head_line {
        (anchor.line..=head_line).collect()
    } else {
        (head_line..=anchor.line).rev().collect()
    };
    let selections: Vec<Selection> = lines
        .iter()
        .filter_map(|&line| {
            let content = text.line_content(line);
            let line_visual_width = indentation.visual_column(&content, content.chars().count());
            if line_visual_width < anchor_visual_column.min(head_visual_column) && line != anchor.line {
                return None;
            }
            let line_start = text.line_start(line);
            let anchor_column = indentation.column_at_visual(&content, anchor_visual_column);
            let head_column = indentation.column_at_visual(&content, head_visual_column);
            Some(
                Selection::new(line_start + anchor_column, line_start + head_column)
                    .with_preferred_visual_column(Some(head_visual_column)),
            )
        })
        .collect();
    if selections.is_empty() {
        return Selections::cursor(text.offset(anchor));
    }
    let primary_index = selections.len() - 1;
    Selections::new(selections, primary_index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_d_selects_the_word_then_adds_next_occurrences() {
        let text = Text::new("foo bar foo baz foo");
        let config = EditingConfig::default();
        let start = Selections::cursor(1);

        let word = add_next_occurrence(&text, &config, &start);
        assert_eq!(word.as_slice(), &[Selection::new(0, 3)]);
        let two = add_next_occurrence(&text, &config, &word);
        assert_eq!(two.as_slice(), &[Selection::new(0, 3), Selection::new(8, 11)]);
        let three = add_next_occurrence(&text, &config, &two);
        assert_eq!(three.len(), 3);
        assert_eq!(three.primary(), Selection::new(16, 19));
        assert_eq!(add_next_occurrence(&text, &config, &three), three);
    }

    #[test]
    fn select_all_occurrences_keeps_the_primary() {
        let text = Text::new("a.b a.b a.b");
        let config = EditingConfig::default();
        let selections = Selections::single(Selection::new(4, 7));

        let all = select_all_occurrences(&text, &config, &selections);

        assert_eq!(all.len(), 3);
        assert_eq!(all.primary(), Selection::new(4, 7));
    }

    #[test]
    fn occurrences_never_start_inside_a_crlf() {
        let text = Text::new("a\nb\r\nc\n");
        let config = EditingConfig::default();
        let newline = Selections::single(Selection::new(1, 2));

        let all = select_all_occurrences(&text, &config, &newline);

        let starts: Vec<usize> = all.iter().map(Selection::start).collect();
        assert_eq!(starts, vec![1, 6]);
    }

    #[test]
    fn occurrences_count_chars_not_bytes() {
        assert_eq!(occurrence_offsets("é x é x", "x"), vec![2, 6]);
    }

    #[test]
    fn adding_cursors_below_extends_the_column() {
        let text = Text::new("abc\nabc\nabc");
        let config = EditingConfig::default();
        let start = Selections::cursor(1);

        let two = add_cursors_vertically(&text, &config, &start, VerticalDirection::Down);
        let three = add_cursors_vertically(&text, &config, &two, VerticalDirection::Down);
        let still_three = add_cursors_vertically(&text, &config, &three, VerticalDirection::Down);

        let heads: Vec<usize> = still_three.iter().map(|selection| selection.head).collect();
        assert_eq!(heads, vec![1, 5, 9]);
    }

    #[test]
    fn ctrl_l_selects_lines_and_grows() {
        let text = Text::new("a\nb\nc");
        let once = expand_to_lines(&text, &Selections::cursor(0));
        assert_eq!(once.as_slice(), &[Selection::new(0, 2)]);
        let twice = expand_to_lines(&text, &once);
        assert_eq!(twice.as_slice(), &[Selection::new(0, 4)]);
    }

    #[test]
    fn column_selection_spans_lines() {
        let text = Text::new("abcdef\nab\nabcdef");
        let config = EditingConfig::default();

        let selections = column_selection(&text, &config, Position::new(0, 1), 2, 4);

        let ranges: Vec<(Position, Position)> = selections
            .iter()
            .map(|selection| (text.position(selection.anchor), text.position(selection.head)))
            .collect();
        assert_eq!(
            ranges,
            vec![
                (Position::new(0, 1), Position::new(0, 4)),
                (Position::new(1, 1), Position::new(1, 2)),
                (Position::new(2, 1), Position::new(2, 4)),
            ]
        );
    }
}
