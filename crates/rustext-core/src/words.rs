//! Grapheme and word boundaries inside one line, in char columns.

use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

/// Monaco's default `wordSeparators`.
pub const DEFAULT_WORD_SEPARATORS: &str = "`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CharClass {
    Whitespace,
    Separator,
    Word,
}

/// Classifies chars into words, separators and whitespace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WordDefinition {
    separators: String,
}

impl WordDefinition {
    pub fn new(separators: impl Into<String>) -> Self {
        Self { separators: separators.into() }
    }

    pub fn class_of(&self, character: char) -> CharClass {
        if character.is_whitespace() {
            CharClass::Whitespace
        } else if self.separators.contains(character) {
            CharClass::Separator
        } else {
            CharClass::Word
        }
    }

    /// Column where the word run (same class, not whitespace) ending at or after `column` starts,
    /// skipping whitespace before `column` first. `None` at column 0.
    pub fn previous_word_start(&self, line: &[char], column: usize) -> Option<usize> {
        let mut index = column.min(line.len());
        while index > 0 && self.class_of(line[index - 1]) == CharClass::Whitespace {
            index -= 1;
        }
        if index == 0 {
            return (column > 0).then_some(0);
        }
        let class = self.class_of(line[index - 1]);
        while index > 0 && self.class_of(line[index - 1]) == class {
            index -= 1;
        }
        Some(index)
    }

    /// Column where the word run after `column` ends, skipping whitespace first.
    /// `None` at the end of the line.
    pub fn next_word_end(&self, line: &[char], column: usize) -> Option<usize> {
        let mut index = column;
        if index >= line.len() {
            return None;
        }
        while index < line.len() && self.class_of(line[index]) == CharClass::Whitespace {
            index += 1;
        }
        if index == line.len() {
            return Some(index);
        }
        let class = self.class_of(line[index]);
        while index < line.len() && self.class_of(line[index]) == class {
            index += 1;
        }
        Some(index)
    }

    /// The word (letters, digits, `_`… anything but separators and whitespace) touching
    /// `column`, as a column range. Prefers the word after the column when the column is
    /// between two words.
    pub fn word_at(&self, line: &[char], column: usize) -> Option<(usize, usize)> {
        let is_word_at = |index: usize| index < line.len() && self.class_of(line[index]) == CharClass::Word;
        let seed = if is_word_at(column) {
            column
        } else if column > 0 && is_word_at(column - 1) {
            column - 1
        } else {
            return None;
        };
        let mut start = seed;
        while start > 0 && is_word_at(start - 1) {
            start -= 1;
        }
        let mut end = seed;
        while is_word_at(end) {
            end += 1;
        }
        Some((start, end))
    }

    /// Same as `word_at`, but a run of separators also counts (double-click on `->` selects it).
    pub fn run_at(&self, line: &[char], column: usize) -> Option<(usize, usize)> {
        let seed = if column < line.len() {
            column
        } else if column > 0 {
            column - 1
        } else {
            return None;
        };
        let class = self.class_of(line[seed]);
        let mut start = seed;
        while start > 0 && self.class_of(line[start - 1]) == class {
            start -= 1;
        }
        let mut end = seed;
        while end < line.len() && self.class_of(line[end]) == class {
            end += 1;
        }
        Some((start, end))
    }
}

impl Default for WordDefinition {
    fn default() -> Self {
        Self::new(DEFAULT_WORD_SEPARATORS)
    }
}

/// A word part boundary: `camelCase`, `snake_case`, `kebab-case`, digits.
fn starts_word_part(previous: char, current: char) -> bool {
    let separator = |character: char| character == '_' || character == '-' || character.is_whitespace();
    (previous.is_lowercase() && current.is_uppercase())
        || (separator(previous) && !separator(current))
        || (previous.is_alphabetic() && current.is_ascii_digit())
        || (previous.is_ascii_digit() && current.is_alphabetic())
        || (!previous.is_alphanumeric() && !separator(previous) && current.is_alphanumeric())
}

pub fn previous_word_part_start(line: &[char], column: usize) -> Option<usize> {
    if column == 0 {
        return None;
    }
    let mut index = column.min(line.len()) - 1;
    while index > 0 && !starts_word_part(line[index - 1], line[index]) {
        index -= 1;
    }
    Some(index)
}

pub fn next_word_part_start(line: &[char], column: usize) -> Option<usize> {
    if column >= line.len() {
        return None;
    }
    let mut index = column + 1;
    while index < line.len() && !starts_word_part(line[index - 1], line[index]) {
        index += 1;
    }
    Some(index)
}

/// Column of the grapheme boundary after `column` in `line`.
pub fn next_grapheme_column(line: &str, column: usize) -> usize {
    let byte_offset = byte_offset_of_column(line, column);
    let mut cursor = GraphemeCursor::new(byte_offset, line.len(), true);
    match cursor.next_boundary(line, 0) {
        Ok(Some(next_byte_offset)) => column + line[byte_offset..next_byte_offset].chars().count(),
        _ => column,
    }
}

/// Column of the grapheme boundary before `column` in `line`.
pub fn previous_grapheme_column(line: &str, column: usize) -> usize {
    let byte_offset = byte_offset_of_column(line, column);
    let mut cursor = GraphemeCursor::new(byte_offset, line.len(), true);
    match cursor.prev_boundary(line, 0) {
        Ok(Some(previous_byte_offset)) => column - line[previous_byte_offset..byte_offset].chars().count(),
        _ => column,
    }
}

/// Number of grapheme clusters in a string.
pub fn grapheme_count(line: &str) -> usize {
    line.graphemes(true).count()
}

pub fn byte_offset_of_column(line: &str, column: usize) -> usize {
    line.char_indices().nth(column).map_or(line.len(), |(byte_offset, _)| byte_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(line: &str) -> Vec<char> {
        line.chars().collect()
    }

    #[test]
    fn word_motions_stop_at_class_changes() {
        let words = WordDefinition::default();
        let line = chars("let foo_bar = a.b();");

        assert_eq!(words.next_word_end(&line, 0), Some(3));
        assert_eq!(words.next_word_end(&line, 3), Some(11));
        assert_eq!(words.next_word_end(&line, 11), Some(13));
        assert_eq!(words.previous_word_start(&line, 11), Some(4));
        assert_eq!(words.previous_word_start(&line, 4), Some(0));
        assert_eq!(words.previous_word_start(&line, 20), Some(17));
        assert_eq!(words.previous_word_start(&line, 0), None);
        assert_eq!(words.next_word_end(&line, 20), None);
    }

    #[test]
    fn word_at_prefers_the_word_after() {
        let words = WordDefinition::default();
        let line = chars("ab.cd");

        assert_eq!(words.word_at(&line, 2), Some((0, 2)));
        assert_eq!(words.word_at(&line, 3), Some((3, 5)));
        assert_eq!(words.word_at(&line, 5), Some((3, 5)));
        assert_eq!(words.word_at(&chars("  "), 1), None);
    }

    #[test]
    fn word_parts_split_camel_and_snake_case() {
        let line = chars("fooBarBaz_qux2");

        assert_eq!(next_word_part_start(&line, 0), Some(3));
        assert_eq!(next_word_part_start(&line, 3), Some(6));
        assert_eq!(next_word_part_start(&line, 6), Some(10));
        assert_eq!(previous_word_part_start(&line, 10), Some(6));
        assert_eq!(previous_word_part_start(&line, 14), Some(13));
    }

    #[test]
    fn grapheme_steps_skip_combining_marks_and_emoji_sequences() {
        let line = "e\u{301}x👍🏽y";

        assert_eq!(next_grapheme_column(line, 0), 2);
        assert_eq!(next_grapheme_column(line, 3), 5);
        assert_eq!(previous_grapheme_column(line, 5), 3);
        assert_eq!(previous_grapheme_column(line, 2), 0);
        assert_eq!(previous_grapheme_column(line, 0), 0);
    }
}
