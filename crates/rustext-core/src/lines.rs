//! Line operations: move, copy, delete, join, sort, trim, insert line, case transforms.

use crate::change::{Bias, Change, ChangeSet};
use crate::cursors::{VerticalDirection, word_range_at};
use crate::edit::{EditContext, Replacement, Transaction, covered_lines, replace_each, whole_line_offsets};
use crate::history::EditKind;
use crate::indentation::leading_whitespace;
use crate::position::Position;
use crate::selection::{Selection, Selections};
use crate::text::Text;

/// Runs of consecutive covered lines, as `(first_line, last_line)`.
pub fn line_blocks(text: &Text, selections: &Selections) -> Vec<(usize, usize)> {
    let mut blocks: Vec<(usize, usize)> = Vec::new();
    for line in covered_lines(text, selections) {
        match blocks.last_mut() {
            Some((_, last_line)) if line <= *last_line + 1 => *last_line = (*last_line).max(line),
            _ => blocks.push((line, line)),
        }
    }
    blocks
}

/// Builds a transaction whose selections are placed by position in the new text:
/// `new_position(first_covered_line, position)` turns an old position of a selection whose
/// covered lines start at `first_covered_line` into the new position.
fn transaction_by_position(
    context: &EditContext,
    changes: ChangeSet,
    new_position: impl Fn(usize, Position) -> Position,
) -> Option<Transaction> {
    if changes.is_empty() {
        return None;
    }
    let (text, new_text) = (context.text, changes.apply(context.text));
    let selections = context.selections.map_each(|selection| {
        let (first_covered_line, _) = selection.covered_lines(text);
        let anchor = new_text.offset(new_position(first_covered_line, text.position(selection.anchor)));
        let head = new_text.offset(new_position(first_covered_line, text.position(selection.head)));
        Selection::new(anchor, head)
    });
    Some(Transaction { changes, selections, kind: EditKind::Other, auto_closed_offsets: Vec::new() })
}

/// The lines `first..=last` joined with the text's line ending.
fn joined_lines(text: &Text, lines: impl Iterator<Item = usize>) -> String {
    lines.map(|line| text.line_content(line)).collect::<Vec<_>>().join(text.line_ending().as_str())
}

/// Replacement of the content of lines `first..=last` (line endings between them included,
/// the one after `last` kept).
fn lines_change(text: &Text, first_line: usize, last_line: usize, new_content: String) -> Change {
    Change::new(text.line_start(first_line), text.line_content_end(last_line), new_content)
}

/// Alt+Up / Alt+Down: the covered lines swap with the line above or below.
pub fn move_lines(context: &EditContext, direction: VerticalDirection) -> Option<Transaction> {
    let text = context.text;
    let blocks = line_blocks(text, context.selections);
    let is_blocked = match direction {
        VerticalDirection::Up => blocks.first().is_some_and(|&(first_line, _)| first_line == 0),
        VerticalDirection::Down => blocks.last().is_some_and(|&(_, last_line)| last_line == text.last_line()),
    };
    if is_blocked {
        return None;
    }

    let changes = ChangeSet::new(blocks.iter().map(|&(first_line, last_line)| match direction {
        VerticalDirection::Up => {
            let lines = (first_line..=last_line).chain([first_line - 1]);
            lines_change(text, first_line - 1, last_line, joined_lines(text, lines))
        }
        VerticalDirection::Down => {
            let lines = [last_line + 1].into_iter().chain(first_line..=last_line);
            lines_change(text, first_line, last_line + 1, joined_lines(text, lines))
        }
    }));
    let line_delta: isize = if direction == VerticalDirection::Up { -1 } else { 1 };
    transaction_by_position(context, changes, |_, position| {
        Position::new(position.line.saturating_add_signed(line_delta), position.column)
    })
}

/// Shift+Alt+Up / Down: the covered lines are duplicated above or below; the cursors go to
/// the copy in that direction.
pub fn copy_lines(context: &EditContext, direction: VerticalDirection) -> Option<Transaction> {
    let text = context.text;
    let line_ending = text.line_ending().as_str();
    let blocks = line_blocks(text, context.selections);
    let changes = ChangeSet::new(blocks.iter().map(|&(first_line, last_line)| {
        let copied = joined_lines(text, first_line..=last_line);
        match direction {
            VerticalDirection::Up => Change::insert(text.line_start(first_line), format!("{copied}{line_ending}")),
            VerticalDirection::Down => {
                Change::insert(text.line_content_end(last_line), format!("{line_ending}{copied}"))
            }
        }
    }));
    transaction_by_position(context, changes, |first_covered_line, position| {
        let lines_added_before: usize = blocks
            .iter()
            .filter(|&&(first_line, _)| first_line <= first_covered_line)
            .map(|&(first_line, last_line)| {
                let is_own_block = (first_line..=last_line).contains(&first_covered_line);
                if is_own_block && direction == VerticalDirection::Up { 0 } else { last_line - first_line + 1 }
            })
            .sum();
        Position::new(position.line + lines_added_before, position.column)
    })
}

/// Ctrl+Shift+K: deletes the covered lines; cursors keep their column on the line now there.
pub fn delete_lines(context: &EditContext) -> Option<Transaction> {
    let text = context.text;
    let blocks = line_blocks(text, context.selections);
    let changes = ChangeSet::new(blocks.iter().map(|&(first_line, last_line)| {
        let (start, _) = whole_line_offsets(text, first_line);
        let (_, end) = whole_line_offsets(text, last_line);
        Change::delete(start, end)
    }));
    if changes.is_empty() {
        return None;
    }
    let new_text = changes.apply(text);
    let selections = context.selections.map_each(|selection| {
        let column = text.position(selection.head).column;
        let new_line = new_text.line_of_offset(changes.map_offset(selection.head, Bias::After));
        Selection::cursor(new_text.offset(Position::new(new_line, column)))
    });
    Some(Transaction { changes, selections, kind: EditKind::Other, auto_closed_offsets: Vec::new() })
}

/// Duplicates each selection after itself and selects the copy; an empty selection copies its line.
pub fn duplicate_selection(context: &EditContext) -> Option<Transaction> {
    if context.selections.iter().all(Selection::is_empty) {
        return copy_lines(context, VerticalDirection::Down);
    }
    let text = context.text;
    replace_each(context, EditKind::Other, |selection| {
        if selection.is_empty() {
            return None;
        }
        let copied = text.slice(selection.start(), selection.end());
        let copied_len = selection.end() - selection.start();
        Some(Replacement::new(selection.end(), selection.end(), copied).with_selection(0, copied_len))
    })
}

/// Joins each selection's lines (or the cursor's line with the next) with single spaces.
pub fn join_lines(context: &EditContext) -> Option<Transaction> {
    let text = context.text;
    replace_each(context, EditKind::Other, |selection| {
        let (first_line, last_line) = selection.covered_lines(text);
        let last_line = last_line.max(first_line + 1).min(text.last_line());
        if first_line == last_line {
            return None;
        }
        let mut joined = text.line_content(first_line).trim_end().to_string();
        for line in first_line + 1..=last_line {
            let content = text.line_content(line);
            let trimmed = content.trim();
            if !trimmed.is_empty() && !joined.is_empty() {
                joined.push(' ');
            }
            joined.push_str(trimmed);
        }
        let first_line_len = text.line_content(first_line).trim_end().chars().count();
        let start = text.line_start(first_line);
        let replacement = Replacement::new(start, text.line_content_end(last_line), joined);
        Some(if selection.is_empty() { replacement.with_cursor(first_line_len) } else { replacement })
    })
}

/// How `rewrite_lines` changes a block of lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinesRewrite {
    SortAscending,
    SortDescending,
    Reverse,
    RemoveDuplicates,
}

/// Sorts, reverses or deduplicates each block of covered lines. A block of one line (a single
/// cursor) is left alone.
pub fn rewrite_lines(context: &EditContext, rewrite: LinesRewrite) -> Option<Transaction> {
    let text = context.text;
    let changes =
        ChangeSet::new(line_blocks(text, context.selections).into_iter().filter(|(first, last)| first < last).map(
            |(first_line, last_line)| {
                let mut lines: Vec<String> = (first_line..=last_line).map(|line| text.line_content(line)).collect();
                match rewrite {
                    LinesRewrite::SortAscending => lines.sort(),
                    LinesRewrite::SortDescending => lines.sort_by(|first, second| second.cmp(first)),
                    LinesRewrite::Reverse => lines.reverse(),
                    LinesRewrite::RemoveDuplicates => {
                        let mut seen = std::collections::HashSet::new();
                        lines.retain(|line| seen.insert(line.clone()));
                    }
                }
                lines_change(text, first_line, last_line, lines.join(text.line_ending().as_str()))
            },
        ));
    transaction_by_position(context, changes, |_, position| position)
}

/// Removes whitespace at the end of every line.
pub fn trim_trailing_whitespace(context: &EditContext) -> Option<Transaction> {
    let text = context.text;
    let changes = ChangeSet::new((0..text.line_count()).filter_map(|line| {
        let content = text.line_content(line);
        let trailing_len = content.chars().rev().take_while(|character| character.is_whitespace()).count();
        let content_end = text.line_content_end(line);
        (trailing_len > 0).then(|| Change::delete(content_end - trailing_len, content_end))
    }));
    if changes.is_empty() {
        return None;
    }
    let selections = context.selections.map(&changes);
    Some(Transaction { changes, selections, kind: EditKind::Other, auto_closed_offsets: Vec::new() })
}

/// Ends the text with its line ending when the last line has content (VS Code's
/// `files.insertFinalNewline`, for an application to run before saving). Cursors at the end
/// stay on their line.
pub fn insert_final_newline(context: &EditContext) -> Option<Transaction> {
    let text = context.text;
    if text.line_len(text.last_line()) == 0 {
        return None;
    }
    let changes = ChangeSet::single(Change::insert(text.len_chars(), text.line_ending().as_str()));
    let map = |offset: usize| changes.map_offset(offset, crate::change::Bias::Before);
    let selections =
        context.selections.map_each(|selection| Selection::new(map(selection.anchor), map(selection.head)));
    Some(Transaction { changes, selections, kind: EditKind::Other, auto_closed_offsets: Vec::new() })
}

/// Ctrl+Enter / Ctrl+Shift+Enter: a new line below or above the cursor's line, with its
/// indentation, the cursor on it.
pub fn insert_line(context: &EditContext, direction: VerticalDirection) -> Option<Transaction> {
    let text = context.text;
    let line_ending = text.line_ending().as_str();
    replace_each(context, EditKind::Other, |selection| {
        let line = text.line_of_offset(selection.head);
        let indent = leading_whitespace(&text.line_content(line)).to_string();
        let indent_len = indent.chars().count();
        Some(match direction {
            VerticalDirection::Down => {
                let content_end = text.line_content_end(line);
                Replacement::new(content_end, content_end, format!("{line_ending}{indent}"))
            }
            VerticalDirection::Up => {
                let line_start = text.line_start(line);
                Replacement::new(line_start, line_start, format!("{indent}{line_ending}")).with_cursor(indent_len)
            }
        })
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaseTransform {
    Upper,
    Lower,
    Title,
    Snake,
    Camel,
    Pascal,
    Kebab,
}

/// Transforms each selection (or the word at each cursor) and keeps it selected.
pub fn transform_case(context: &EditContext, transform: CaseTransform) -> Option<Transaction> {
    let text = context.text;
    replace_each(context, EditKind::Other, |selection| {
        let (start, end) = if selection.is_empty() {
            word_range_at(text, context.config, selection.head)?
        } else {
            (selection.start(), selection.end())
        };
        let transformed = apply_case(&text.slice(start, end), transform);
        let transformed_len = transformed.chars().count();
        let (anchor, head) = if selection.is_empty() {
            let cursor = (selection.head - start).min(transformed_len);
            (cursor, cursor)
        } else if selection.is_reversed() {
            (transformed_len, 0)
        } else {
            (0, transformed_len)
        };
        Some(Replacement::new(start, end, transformed).with_selection(anchor, head))
    })
}

/// Splits an identifier or phrase in lowercase words: `fooBar_baz qux` → `foo bar baz qux`.
fn identifier_words(input: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut previous: Option<char> = None;
    for character in input.chars() {
        if !character.is_alphanumeric() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            previous = None;
            continue;
        }
        let starts_word = previous.is_some_and(|previous| previous.is_lowercase() && character.is_uppercase());
        if starts_word && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        current.extend(character.to_lowercase());
        previous = Some(character);
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

pub fn apply_case(input: &str, transform: CaseTransform) -> String {
    match transform {
        CaseTransform::Upper => input.to_uppercase(),
        CaseTransform::Lower => input.to_lowercase(),
        CaseTransform::Title => {
            let mut is_word_start = true;
            input
                .chars()
                .flat_map(|character| {
                    let result: Vec<char> = if is_word_start {
                        character.to_uppercase().collect()
                    } else {
                        character.to_lowercase().collect()
                    };
                    is_word_start = !character.is_alphanumeric();
                    result
                })
                .collect()
        }
        CaseTransform::Snake => identifier_words(input).join("_"),
        CaseTransform::Kebab => identifier_words(input).join("-"),
        CaseTransform::Camel => {
            let words = identifier_words(input);
            words
                .iter()
                .enumerate()
                .map(|(index, word)| if index == 0 { word.clone() } else { capitalized(word) })
                .collect()
        }
        CaseTransform::Pascal => identifier_words(input).iter().map(|word| capitalized(word)).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EditingConfig, LanguageConfiguration};

    fn run(
        content: &str,
        selections: Selections,
        command: impl Fn(&EditContext) -> Option<Transaction>,
    ) -> (String, Selections) {
        let text = Text::new(content);
        let config = EditingConfig::default();
        let language = LanguageConfiguration::default();
        let context = EditContext {
            text: &text,
            selections: &selections,
            config: &config,
            language: &language,
            auto_closed_offsets: &[],
        };
        let transaction = command(&context).expect("a transaction");
        (transaction.changes.apply(&text).to_string(), transaction.selections)
    }

    #[test]
    fn a_final_newline_is_added_after_a_last_line_with_content() {
        let (content, selections) = run("a\r\nb", Selections::cursor(4), insert_final_newline);

        assert_eq!(content, "a\r\nb\r\n");
        assert_eq!(selections.primary(), Selection::cursor(4));
    }

    #[test]
    fn a_text_ending_with_a_newline_gets_no_other() {
        let text = Text::new("a\n");
        let (config, language) = (EditingConfig::default(), LanguageConfiguration::default());
        let selections = Selections::cursor(0);
        let context = EditContext {
            text: &text,
            selections: &selections,
            config: &config,
            language: &language,
            auto_closed_offsets: &[],
        };

        assert!(insert_final_newline(&context).is_none());
    }

    #[test]
    fn moves_a_line_up_with_its_cursor() {
        let (content, selections) =
            run("a\nbc\nd", Selections::cursor(3), |context| move_lines(context, VerticalDirection::Up));

        assert_eq!(content, "bc\na\nd");
        assert_eq!(selections.primary(), Selection::cursor(1));
    }

    #[test]
    fn moves_a_block_down() {
        let (content, selections) = run("a\nb\nc\nd", Selections::single(Selection::new(0, 3)), |context| {
            move_lines(context, VerticalDirection::Down)
        });

        assert_eq!(content, "c\na\nb\nd");
        assert_eq!(selections.primary(), Selection::new(2, 5));
    }

    #[test]
    fn copies_lines_down_and_follows_the_copy() {
        let (content, selections) =
            run("ab\ncd", Selections::cursor(1), |context| copy_lines(context, VerticalDirection::Down));

        assert_eq!(content, "ab\nab\ncd");
        assert_eq!(selections.primary(), Selection::cursor(4));
    }

    #[test]
    fn copies_lines_up_and_stays_on_the_upper_copy() {
        let (content, selections) =
            run("ab\ncd", Selections::cursor(4), |context| copy_lines(context, VerticalDirection::Up));

        assert_eq!(content, "ab\ncd\ncd");
        assert_eq!(selections.primary(), Selection::cursor(4));
    }

    #[test]
    fn deletes_lines_keeping_the_column() {
        let (content, selections) = run("abc\ndef\ngh", Selections::cursor(5), delete_lines);

        assert_eq!(content, "abc\ngh");
        assert_eq!(selections.primary(), Selection::cursor(5));
    }

    #[test]
    fn deleting_the_last_line_removes_the_line_break_before_it() {
        let (content, _) = run("abc\ndef", Selections::cursor(5), delete_lines);

        assert_eq!(content, "abc");
    }

    #[test]
    fn joins_lines_trimming_indentation() {
        let (content, selections) = run("foo  \n    bar\nbaz", Selections::cursor(0), join_lines);

        assert_eq!(content, "foo bar\nbaz");
        assert_eq!(selections.primary(), Selection::cursor(3));
    }

    #[test]
    fn sorts_selected_lines() {
        let (content, _) = run("c\nb\na\nz", Selections::single(Selection::new(0, 5)), |context| {
            rewrite_lines(context, LinesRewrite::SortAscending)
        });

        assert_eq!(content, "a\nb\nc\nz");
    }

    #[test]
    fn trims_trailing_whitespace() {
        let (content, _) = run("a  \nb\t\nc", Selections::cursor(0), trim_trailing_whitespace);

        assert_eq!(content, "a\nb\nc");
    }

    #[test]
    fn inserts_a_line_below_with_indentation() {
        let (content, selections) =
            run("  ab\ncd", Selections::cursor(3), |context| insert_line(context, VerticalDirection::Down));

        assert_eq!(content, "  ab\n  \ncd");
        assert_eq!(selections.primary(), Selection::cursor(7));
    }

    #[test]
    fn case_transforms() {
        assert_eq!(apply_case("fooBar_baz", CaseTransform::Snake), "foo_bar_baz");
        assert_eq!(apply_case("foo_bar baz", CaseTransform::Camel), "fooBarBaz");
        assert_eq!(apply_case("foo-bar", CaseTransform::Pascal), "FooBar");
        assert_eq!(apply_case("FooBar", CaseTransform::Kebab), "foo-bar");
        assert_eq!(apply_case("hello wORLD", CaseTransform::Title), "Hello World");
    }

    #[test]
    fn case_transform_of_the_word_at_the_cursor() {
        let (content, selections) =
            run("say hello", Selections::cursor(6), |context| transform_case(context, CaseTransform::Upper));

        assert_eq!(content, "say HELLO");
        assert_eq!(selections.primary(), Selection::cursor(6));
    }
}
