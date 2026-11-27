//! Line diff with inner (word) changes, as values (DECISIONS D14).
//!
//! Lines are compared with their line ending, as git does: a CRLF ↔ LF change and a missing
//! newline at the end of the file are changes. A text ending with `\n` has no extra empty
//! last line here (`"a\n"` is one line), so line indexes are the `Text`'s and git's.

use std::time::{Duration, Instant};

use imara_diff::{Algorithm, Diff, InternedInput};
use rustext_core::{Position, Range, Text};

use crate::moves::{MovedBlock, detect_moves};

/// Lines `start..end` (0-based, end excluded).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

impl LineRange {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn contains(&self, line: usize) -> bool {
        (self.start..self.end).contains(&line)
    }

    pub fn iter(&self) -> std::ops::Range<usize> {
        self.start..self.end
    }
}

/// A changed region of the old text in `old` became `new`. Inner changes locate the changed
/// words inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub old: LineRange,
    pub new: LineRange,
    pub inner_changes: Vec<InnerChange>,
}

impl Hunk {
    pub fn is_pure_insertion(&self) -> bool {
        self.old.is_empty()
    }

    pub fn is_pure_deletion(&self) -> bool {
        self.new.is_empty()
    }
}

/// A changed run of chars: `old` in the old text became `new` in the new text. One of the two
/// can be empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InnerChange {
    pub old: Range,
    pub new: Range,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffOptions {
    /// Lines differing only by leading or trailing whitespace are equal.
    pub ignore_trim_whitespace: bool,
    /// Computes the changed words inside changed lines.
    pub inner_changes: bool,
    /// Hunks with more lines than this get no inner changes (too slow to be worth it).
    pub max_inner_change_lines: usize,
    /// Past this, inner changes stop being computed for the remaining hunks.
    pub max_computation_time: Duration,
    /// Finds blocks removed in one hunk and added in another (Monaco's
    /// `experimental.showMoves`, off by default as there).
    pub detect_moves: bool,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            ignore_trim_whitespace: false,
            inner_changes: true,
            max_inner_change_lines: 1_000,
            max_computation_time: Duration::from_millis(500),
            detect_moves: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineDiff {
    pub hunks: Vec<Hunk>,
    /// Number of lines (in git's sense) of each side.
    pub old_line_count: usize,
    pub new_line_count: usize,
    /// Inner changes were skipped for some hunks because of `max_computation_time`.
    pub hit_time_limit: bool,
    /// Blocks moved from one hunk to another, by their old lines; with `detect_moves` only.
    pub moves: Vec<MovedBlock>,
}

impl LineDiff {
    pub fn is_identical(&self) -> bool {
        self.hunks.is_empty()
    }

    pub fn added_line_count(&self) -> usize {
        self.hunks.iter().map(|hunk| hunk.new.len()).sum()
    }

    pub fn removed_line_count(&self) -> usize {
        self.hunks.iter().map(|hunk| hunk.old.len()).sum()
    }
}

/// Number of lines of a text in git's sense: a final `\n` ends the last line instead of
/// starting an empty one.
pub fn git_line_count(text: &Text) -> usize {
    let line_count = text.line_count();
    if text.line_len(text.last_line()) == 0 { line_count - 1 } else { line_count }
}

/// A line with its line ending (`\n`, `\r\n`, or nothing for a last line without newline).
pub fn raw_line(text: &Text, line: usize) -> String {
    let ending = text.line_ending_of(line).map_or("", |line_ending| line_ending.as_str());
    text.line_content(line) + ending
}

/// The diff key of each line in git's sense: the raw line, or with `ignore_trim_whitespace`
/// the trimmed content plus whether it ends with a newline.
fn line_keys(text: &Text, ignore_trim_whitespace: bool) -> impl Iterator<Item = String> + '_ {
    text.raw_lines().take(git_line_count(text)).map(move |raw| {
        if !ignore_trim_whitespace {
            return raw;
        }
        let has_newline = raw.ends_with('\n');
        raw.trim().to_string() + if has_newline { "\n" } else { "" }
    })
}

pub fn diff_lines(old: &Text, new: &Text, options: &DiffOptions) -> LineDiff {
    let started_at = Instant::now();
    let old_line_count = git_line_count(old);
    let new_line_count = git_line_count(new);

    let mut input: InternedInput<String> = InternedInput::default();
    input.update_before(line_keys(old, options.ignore_trim_whitespace));
    input.update_after(line_keys(new, options.ignore_trim_whitespace));
    let mut diff = Diff::compute(Algorithm::Myers, &input);
    diff.postprocess_lines(&input);

    let mut hit_time_limit = false;
    let hunks: Vec<Hunk> = diff
        .hunks()
        .map(|hunk| {
            let old_range = LineRange::new(hunk.before.start as usize, hunk.before.end as usize);
            let new_range = LineRange::new(hunk.after.start as usize, hunk.after.end as usize);
            let is_replacement = !old_range.is_empty() && !new_range.is_empty();
            let is_small = old_range.len() + new_range.len() <= options.max_inner_change_lines;
            let has_time = started_at.elapsed() < options.max_computation_time;
            hit_time_limit |= is_replacement && is_small && !has_time;
            let inner_changes = if options.inner_changes && is_replacement && is_small && has_time {
                inner_changes(old, old_range, new, new_range)
            } else {
                Vec::new()
            };
            Hunk { old: old_range, new: new_range, inner_changes }
        })
        .collect();
    let moves = if options.detect_moves { detect_moves(old, new, &hunks) } else { Vec::new() };
    LineDiff { hunks, old_line_count, new_line_count, hit_time_limit, moves }
}

/// A word of a hunk's text: a run of word chars, a run of whitespace, or one other char; with
/// the position it starts at.
struct Word {
    text: String,
    start: Position,
    end: Position,
}

fn words_of(text: &Text, lines: LineRange) -> Vec<Word> {
    #[derive(PartialEq)]
    enum Class {
        Word,
        Space,
        Other,
    }
    let class_of = |character: char| {
        if character.is_alphanumeric() || character == '_' {
            Class::Word
        } else if character == ' ' || character == '\t' {
            Class::Space
        } else {
            Class::Other
        }
    };

    let mut words = Vec::new();
    for line in lines.iter() {
        let chars: Vec<char> = text.line_content(line).chars().collect();
        let mut column = 0;
        while column < chars.len() {
            let class = class_of(chars[column]);
            let start = column;
            column += 1;
            if class != Class::Other {
                while column < chars.len() && class_of(chars[column]) == class {
                    column += 1;
                }
            }
            words.push(Word {
                text: chars[start..column].iter().collect(),
                start: Position::new(line, start),
                end: Position::new(line, column),
            });
        }
        let line_end = Position::new(line, chars.len());
        words.push(Word { text: "\n".into(), start: line_end, end: Position::new(line + 1, 0) });
    }
    words
}

/// Changed words between two line ranges, merged when only one short unchanged word separates
/// them (so `a.b.c` → `x.y.z` is one change, not five).
fn inner_changes(old: &Text, old_lines: LineRange, new: &Text, new_lines: LineRange) -> Vec<InnerChange> {
    let old_words = words_of(old, old_lines);
    let new_words = words_of(new, new_lines);
    let mut input: InternedInput<&str> = InternedInput::default();
    input.update_before(old_words.iter().map(|word| word.text.as_str()));
    input.update_after(new_words.iter().map(|word| word.text.as_str()));
    let mut diff = Diff::compute(Algorithm::Myers, &input);
    diff.postprocess_no_heuristic(&input);

    let range_of = |words: &[Word], tokens: &std::ops::Range<u32>, line_range: LineRange| -> Range {
        let (start, end) = (tokens.start as usize, tokens.end as usize);
        let fallback = Position::new(line_range.end, 0);
        let start_position = words.get(start).map_or(fallback, |word| word.start);
        let end_position = if end > start { words[end - 1].end } else { start_position };
        Range::new(start_position, end_position)
    };

    let mut changes: Vec<(std::ops::Range<u32>, std::ops::Range<u32>)> = Vec::new();
    for hunk in diff.hunks() {
        if let Some((last_before, last_after)) = changes.last_mut() {
            let gap_before = hunk.before.start - last_before.end;
            let gap_after = hunk.after.start - last_after.end;
            let gap_is_one_short_word = gap_before == 1
                && gap_after == 1
                && old_words.get(last_before.end as usize).is_some_and(|word| word.text.chars().count() <= 1);
            if gap_is_one_short_word {
                last_before.end = hunk.before.end;
                last_after.end = hunk.after.end;
                continue;
            }
        }
        changes.push((hunk.before, hunk.after));
    }

    changes
        .iter()
        .map(|(before, after)| InnerChange {
            old: range_of(&old_words, before, old_lines),
            new: range_of(&new_words, after, new_lines),
        })
        .map(|change| InnerChange { old: clip_line_ending(change.old), new: clip_line_ending(change.new) })
        .collect()
}

/// A change ending at the start of the next line ends at the end of its last line instead, so it
/// paints on the lines it is about.
fn clip_line_ending(range: Range) -> Range {
    if range.end.column == 0 && range.end.line > range.start.line {
        return Range::new(range.start, Position::new(range.end.line - 1, usize::MAX));
    }
    range
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diff(old: &str, new: &str) -> LineDiff {
        diff_lines(&Text::new(old), &Text::new(new), &DiffOptions::default())
    }

    fn ranges(diff: &LineDiff) -> Vec<((usize, usize), (usize, usize))> {
        diff.hunks.iter().map(|hunk| ((hunk.old.start, hunk.old.end), (hunk.new.start, hunk.new.end))).collect()
    }

    #[test]
    fn identical_texts_have_no_hunks() {
        assert!(diff("a\nb\n", "a\nb\n").is_identical());
    }

    #[test]
    fn finds_insertions_deletions_and_replacements() {
        let result = diff("a\nb\nc\nd\n", "a\nB\nc\nd\ne\n");

        assert_eq!(ranges(&result), vec![((1, 2), (1, 2)), ((4, 4), (4, 5))]);
        assert_eq!((result.added_line_count(), result.removed_line_count()), (2, 1));
    }

    #[test]
    fn line_ending_changes_are_changes() {
        assert_eq!(ranges(&diff("a\nb\n", "a\r\nb\n")), vec![((0, 1), (0, 1))]);
    }

    #[test]
    fn missing_final_newline_is_a_change() {
        assert_eq!(ranges(&diff("a\nb\n", "a\nb")), vec![((1, 2), (1, 2))]);
    }

    #[test]
    fn ignoring_trim_whitespace() {
        let options = DiffOptions { ignore_trim_whitespace: true, ..DiffOptions::default() };

        let result = diff_lines(&Text::new("  a\nb\n"), &Text::new("a  \nb\n"), &options);

        assert!(result.is_identical());
    }

    #[test]
    fn git_line_counts() {
        assert_eq!(git_line_count(&Text::new("")), 0);
        assert_eq!(git_line_count(&Text::new("a")), 1);
        assert_eq!(git_line_count(&Text::new("a\n")), 1);
        assert_eq!(git_line_count(&Text::new("a\n\n")), 2);
    }

    #[test]
    fn inner_changes_locate_changed_words() {
        let result = diff("let value = 1;\n", "let total = 1;\n");

        let change = result.hunks[0].inner_changes[0];
        assert_eq!(change.old, Range::new(Position::new(0, 4), Position::new(0, 9)));
        assert_eq!(change.new, Range::new(Position::new(0, 4), Position::new(0, 9)));
        assert_eq!(result.hunks[0].inner_changes.len(), 1);
    }

    #[test]
    fn inner_changes_merge_across_one_char_separators() {
        let result = diff("call(a.b.c)\n", "call(x.y.z)\n");

        let changes = &result.hunks[0].inner_changes;
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].new, Range::new(Position::new(0, 5), Position::new(0, 10)));
    }

    #[test]
    fn inner_insertion_is_empty_on_the_old_side() {
        let result = diff("f(a)\n", "f(a, b)\n");

        let change = result.hunks[0].inner_changes[0];
        assert!(change.old.is_empty());
        assert_eq!(change.new, Range::new(Position::new(0, 3), Position::new(0, 6)));
    }
}
