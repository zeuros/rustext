//! Cursor motions: pure functions from a selection to the selection after moving its head.

use crate::config::EditingConfig;
use crate::indentation::leading_whitespace_len;
use crate::selection::Selection;
use crate::text::Text;
use crate::words::{next_grapheme_column, next_word_part_start, previous_grapheme_column, previous_word_part_start};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    /// To the start of the previous word (Ctrl+Left).
    WordLeft,
    /// To the end of the next word (Ctrl+Right).
    WordRight,
    /// To the previous `camelCase` / `snake_case` part (Alt+Left in VS Code on macOS).
    WordPartLeft,
    WordPartRight,
    /// First non-whitespace char, or column 0 when already there (Home).
    LineStart,
    LineEnd,
    DocumentStart,
    DocumentEnd,
    PageUp {
        lines: usize,
    },
    PageDown {
        lines: usize,
    },
}

impl Motion {
    pub fn is_vertical(self) -> bool {
        matches!(self, Motion::Up | Motion::Down | Motion::PageUp { .. } | Motion::PageDown { .. })
    }
}

/// Lines a view hides (folded regions), as inclusive ranges: vertical moves step over them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HiddenLines {
    ranges: Vec<(usize, usize)>,
}

impl HiddenLines {
    pub fn new(mut ranges: Vec<(usize, usize)>) -> Self {
        ranges.sort_unstable();
        Self { ranges }
    }

    pub fn ranges(&self) -> &[(usize, usize)] {
        &self.ranges
    }

    pub fn is_hidden(&self, line: usize) -> bool {
        self.ranges.iter().any(|&(first_line, last_line)| (first_line..=last_line).contains(&line))
    }

    /// The line `line_delta` shown lines away from `line`, or `None` past the text's edges.
    fn shown_line_after(&self, line: usize, line_delta: isize, last_line: usize) -> Option<usize> {
        let step: isize = if line_delta < 0 { -1 } else { 1 };
        let mut current = line;
        for _ in 0..line_delta.unsigned_abs() {
            loop {
                current = current.checked_add_signed(step).filter(|&next| next <= last_line)?;
                if !self.is_hidden(current) {
                    break;
                }
            }
        }
        Some(current)
    }
}

/// The selection after moving its head by `motion`. Without `extend`, a non-empty selection
/// collapses to its start on Left and to its end on Right, as in every editor.
pub fn apply_motion(
    text: &Text,
    config: &EditingConfig,
    selection: &Selection,
    motion: Motion,
    extend: bool,
) -> Selection {
    apply_motion_over(text, config, &HiddenLines::default(), selection, motion, extend)
}

/// `apply_motion` for a view hiding some lines: vertical moves step over them.
pub fn apply_motion_over(
    text: &Text,
    config: &EditingConfig,
    hidden_lines: &HiddenLines,
    selection: &Selection,
    motion: Motion,
    extend: bool,
) -> Selection {
    if !extend && !selection.is_empty() {
        match motion {
            Motion::Left => return Selection::cursor(selection.start()),
            Motion::Right => return Selection::cursor(selection.end()),
            _ => {}
        }
    }

    match motion {
        Motion::Up => vertical(text, config, hidden_lines, selection, -1, extend),
        Motion::Down => vertical(text, config, hidden_lines, selection, 1, extend),
        Motion::PageUp { lines } => vertical(text, config, hidden_lines, selection, -(lines.max(1) as isize), extend),
        Motion::PageDown { lines } => vertical(text, config, hidden_lines, selection, lines.max(1) as isize, extend),
        _ => selection.moved_to(horizontal_head(text, config, selection.head, motion), extend),
    }
}

fn horizontal_head(text: &Text, config: &EditingConfig, head: usize, motion: Motion) -> usize {
    let position = text.position(head);
    let line = text.line_content(position.line);
    let line_start = text.line_start(position.line);
    let at_line_start = position.column == 0;
    let at_line_end = position.column == text.line_len(position.line);
    let previous_line_end = || text.line_content_end(position.line.saturating_sub(1));
    let next_line_start = || text.line_start(position.line + 1);
    let is_first_line = position.line == 0;
    let is_last_line = position.line == text.last_line();

    match motion {
        Motion::Left if at_line_start => {
            if is_first_line {
                head
            } else {
                previous_line_end()
            }
        }
        Motion::Left => line_start + previous_grapheme_column(&line, position.column),
        Motion::Right if at_line_end => {
            if is_last_line {
                head
            } else {
                next_line_start()
            }
        }
        Motion::Right => line_start + next_grapheme_column(&line, position.column),
        Motion::WordLeft | Motion::WordPartLeft if at_line_start => {
            if is_first_line {
                head
            } else {
                previous_line_end()
            }
        }
        Motion::WordRight | Motion::WordPartRight if at_line_end => {
            if is_last_line {
                head
            } else {
                next_line_start()
            }
        }
        Motion::WordLeft => {
            let chars: Vec<char> = line.chars().collect();
            line_start + config.words.previous_word_start(&chars, position.column).unwrap_or(0)
        }
        Motion::WordRight => {
            let chars: Vec<char> = line.chars().collect();
            line_start + config.words.next_word_end(&chars, position.column).unwrap_or(chars.len())
        }
        Motion::WordPartLeft => {
            let chars: Vec<char> = line.chars().collect();
            line_start + previous_word_part_start(&chars, position.column).unwrap_or(0)
        }
        Motion::WordPartRight => {
            let chars: Vec<char> = line.chars().collect();
            line_start + next_word_part_start(&chars, position.column).unwrap_or(chars.len())
        }
        Motion::LineStart => {
            let first_non_whitespace_column = leading_whitespace_len(&line);
            let column = if position.column == first_non_whitespace_column { 0 } else { first_non_whitespace_column };
            line_start + column
        }
        Motion::LineEnd => text.line_content_end(position.line),
        Motion::DocumentStart => 0,
        Motion::DocumentEnd => text.len_chars(),
        Motion::Up | Motion::Down | Motion::PageUp { .. } | Motion::PageDown { .. } => head,
    }
}

/// Moves the head `line_delta` lines, keeping the visual column the cursor had when the
/// vertical moves started. Moving up from the first line goes to its start, down from the last
/// line to its end.
fn vertical(
    text: &Text,
    config: &EditingConfig,
    hidden_lines: &HiddenLines,
    selection: &Selection,
    line_delta: isize,
    extend: bool,
) -> Selection {
    let position = text.position(selection.head);
    let indentation = config.indentation;
    let preferred_visual_column = selection
        .preferred_visual_column
        .unwrap_or_else(|| indentation.visual_column(&text.line_content(position.line), position.column));

    let head = match hidden_lines.shown_line_after(position.line, line_delta, text.last_line()) {
        Some(target_line) => {
            let column = indentation.column_at_visual(&text.line_content(target_line), preferred_visual_column);
            text.line_start(target_line) + column
        }
        None if line_delta < 0 => 0,
        None => text.len_chars(),
    };
    selection.moved_to(head, extend).with_preferred_visual_column(Some(preferred_visual_column))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::Position;

    fn moved(content: &str, head: Position, motion: Motion) -> Position {
        let text = Text::new(content);
        let selection = Selection::cursor(text.offset(head));
        let after = apply_motion(&text, &EditingConfig::default(), &selection, motion, false);
        text.position(after.head)
    }

    #[test]
    fn left_and_right_wrap_across_crlf_lines() {
        assert_eq!(moved("ab\r\ncd", Position::new(1, 0), Motion::Left), Position::new(0, 2));
        assert_eq!(moved("ab\r\ncd", Position::new(0, 2), Motion::Right), Position::new(1, 0));
        assert_eq!(moved("ab", Position::new(0, 0), Motion::Left), Position::new(0, 0));
    }

    #[test]
    fn vertical_moves_keep_the_visual_column() {
        let text = Text::new("long line\nab\nlong line");
        let config = EditingConfig::default();
        let start = Selection::cursor(text.offset(Position::new(0, 7)));

        let down = apply_motion(&text, &config, &start, Motion::Down, false);
        assert_eq!(text.position(down.head), Position::new(1, 2));
        let down_again = apply_motion(&text, &config, &down, Motion::Down, false);
        assert_eq!(text.position(down_again.head), Position::new(2, 7));
    }

    #[test]
    fn up_from_first_line_goes_to_document_start() {
        assert_eq!(moved("abc\nd", Position::new(0, 2), Motion::Up), Position::new(0, 0));
        assert_eq!(moved("abc\nd", Position::new(1, 0), Motion::Down), Position::new(1, 1));
    }

    #[test]
    fn home_toggles_between_indentation_and_column_zero() {
        assert_eq!(moved("    abc", Position::new(0, 6), Motion::LineStart), Position::new(0, 4));
        assert_eq!(moved("    abc", Position::new(0, 4), Motion::LineStart), Position::new(0, 0));
        assert_eq!(moved("    abc", Position::new(0, 0), Motion::LineStart), Position::new(0, 4));
    }

    #[test]
    fn vertical_moves_step_over_hidden_lines() {
        let text = Text::new("a\nb\nc\nd");
        let hidden_lines = HiddenLines::new(vec![(1, 2)]);
        let config = EditingConfig::default();

        let down = apply_motion_over(&text, &config, &hidden_lines, &Selection::cursor(0), Motion::Down, false);
        assert_eq!(text.position(down.head), Position::new(3, 0));
        let up = apply_motion_over(&text, &config, &hidden_lines, &down, Motion::Up, false);
        assert_eq!(text.position(up.head), Position::new(0, 0));
    }

    #[test]
    fn left_collapses_a_selection_to_its_start() {
        let text = Text::new("abcdef");
        let selection = Selection::new(1, 4);

        let after = apply_motion(&text, &EditingConfig::default(), &selection, Motion::Left, false);

        assert_eq!(after, Selection::cursor(1));
    }

    #[test]
    fn word_moves_cross_lines() {
        assert_eq!(moved("ab\ncd", Position::new(0, 2), Motion::WordRight), Position::new(1, 0));
        assert_eq!(moved("ab\n  cd", Position::new(1, 0), Motion::WordLeft), Position::new(0, 2));
        assert_eq!(moved("foo bar", Position::new(0, 7), Motion::WordLeft), Position::new(0, 4));
    }
}
