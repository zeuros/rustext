//! Editing commands as pure functions: each reads an `EditContext` and returns the
//! `Transaction` to apply, or `None` when there is nothing to do (D7).

use crate::change::{Bias, Change, ChangeSet};
use crate::config::{AutoClosing, EditingConfig, LanguageConfiguration};
use crate::history::EditKind;
use crate::indentation::leading_whitespace;
use crate::selection::{Selection, Selections};
use crate::text::Text;
use crate::words::{CharClass, next_grapheme_column, previous_grapheme_column};

/// What an editing function reads. Borrowed from the editor state.
#[derive(Clone, Copy)]
pub struct EditContext<'a> {
    pub text: &'a Text,
    pub selections: &'a Selections,
    pub config: &'a EditingConfig,
    pub language: &'a LanguageConfiguration,
    /// Offsets of closing chars the editor inserted itself, which typing them steps over.
    pub auto_closed_offsets: &'a [usize],
}

/// A change set, the selections after it, and how history groups it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transaction {
    pub changes: ChangeSet,
    pub selections: Selections,
    pub kind: EditKind,
    /// Offsets, in the new text, of closing chars this transaction auto-inserted.
    pub auto_closed_offsets: Vec<usize>,
}

/// One selection's edit: replace `start..end` with `insert`, then select `anchor..head`, given
/// relative to `start` in the new text (they may point past the inserted text).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replacement {
    pub start: usize,
    pub end: usize,
    pub insert: String,
    pub anchor: usize,
    pub head: usize,
    /// Relative offset of an auto-inserted closing char, like `anchor`.
    pub auto_closed: Option<usize>,
}

impl Replacement {
    /// Replaces `start..end` and puts the cursor after the inserted text.
    pub fn new(start: usize, end: usize, insert: impl Into<String>) -> Self {
        let insert = insert.into();
        let inserted_len = insert.chars().count();
        Self { start, end, insert, anchor: inserted_len, head: inserted_len, auto_closed: None }
    }

    pub fn with_cursor(self, relative_offset: usize) -> Self {
        Self { anchor: relative_offset, head: relative_offset, ..self }
    }

    pub fn with_selection(self, anchor: usize, head: usize) -> Self {
        Self { anchor, head, ..self }
    }
}

/// Builds a transaction from one optional replacement per selection. Selections without a
/// replacement follow the edit.
pub fn replace_each(
    context: &EditContext,
    kind: EditKind,
    mut replacement_for: impl FnMut(&Selection) -> Option<Replacement>,
) -> Option<Transaction> {
    let replacements: Vec<Option<Replacement>> = context.selections.iter().map(&mut replacement_for).collect();
    if replacements.iter().all(Option::is_none) {
        return None;
    }
    let changes = ChangeSet::new(
        replacements
            .iter()
            .flatten()
            .map(|replacement| Change::new(replacement.start, replacement.end, replacement.insert.clone())),
    );

    let mut auto_closed_offsets = Vec::new();
    let selections = context.selections.as_slice().iter().zip(&replacements).map(|(selection, replacement)| {
        let Some(replacement) = replacement else {
            return selection.map(&changes);
        };
        let new_start = changes.map_offset(replacement.start, Bias::Before);
        if let Some(auto_closed) = replacement.auto_closed {
            auto_closed_offsets.push(new_start + auto_closed);
        }
        Selection::new(new_start + replacement.anchor, new_start + replacement.head)
    });
    let selections = Selections::new(selections.collect(), context.selections.primary_index());
    if changes.is_empty() && &selections == context.selections {
        return None;
    }
    Some(Transaction { changes, selections, kind, auto_closed_offsets })
}

/// Replaces every selection with `typed`, with the auto-closing, overtyping and surrounding
/// rules when one char is typed.
pub fn type_text(context: &EditContext, typed: &str) -> Option<Transaction> {
    let mut typed_chars = typed.chars();
    let single_char = match (typed_chars.next(), typed_chars.next()) {
        (Some(character), None) => Some(character),
        _ => None,
    };
    replace_each(context, EditKind::Typing, |selection| match single_char {
        Some(character) => type_char(context, selection, character),
        None => Some(Replacement::new(selection.start(), selection.end(), typed)),
    })
}

fn type_char(context: &EditContext, selection: &Selection, typed: char) -> Option<Replacement> {
    let (text, config, language) = (context.text, context.config, context.language);
    let (start, end) = (selection.start(), selection.end());

    if !selection.is_empty() && config.auto_surround {
        let surrounding_pair = language.surrounding_pairs.iter().find(|(open, _)| *open == typed);
        if let Some(&(open, close)) = surrounding_pair {
            let selected = text.slice(start, end);
            let selected_len = end - start;
            let surrounded = format!("{open}{selected}{close}");
            let (anchor, head) = if selection.is_reversed() { (selected_len + 1, 1) } else { (1, selected_len + 1) };
            return Some(Replacement::new(start, end, surrounded).with_selection(anchor, head));
        }
    }

    let next_char = text.char_at(end).filter(|&character| character != '\n' && character != '\r');
    let steps_over_auto_closed = selection.is_empty()
        && config.auto_closing_overtype
        && next_char == Some(typed)
        && context.auto_closed_offsets.contains(&end);
    if steps_over_auto_closed {
        return Some(Replacement::new(end, end, "").with_cursor(1));
    }

    let pair = language.auto_closing_pairs.iter().find(|pair| pair.open == typed);
    if let Some(pair) = pair.filter(|_| selection.is_empty()) {
        let rule = if pair.is_quote() { config.auto_closing_quotes } else { config.auto_closing_brackets };
        let is_closing_or_whitespace = |character: char| {
            character.is_whitespace() || language.is_closing_bracket(character) || ";,.:".contains(character)
        };
        let allowed_by_next_char = match rule {
            AutoClosing::Always => true,
            AutoClosing::BeforeWhitespace => next_char.is_none_or(is_closing_or_whitespace),
            AutoClosing::Never => false,
        };
        let previous_char = text.chars_before(start).next();
        let is_inside_word = pair.is_quote()
            && previous_char.is_some_and(|character| config.words.class_of(character) == CharClass::Word);
        if allowed_by_next_char && !is_inside_word {
            let closed = format!("{}{}", pair.open, pair.close);
            return Some(Replacement { auto_closed: Some(1), ..Replacement::new(start, end, closed).with_cursor(1) });
        }
    }

    Some(Replacement::new(start, end, typed.to_string()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeleteUnit {
    /// One grapheme, or the indentation back to the previous tab stop.
    Char,
    Word,
    WordPart,
    /// To the start (backward) or end (forward) of the line.
    Line,
}

/// Backspace and its variants. A non-empty selection is deleted whatever the unit.
pub fn delete_backward(context: &EditContext, unit: DeleteUnit) -> Option<Transaction> {
    let text = context.text;
    replace_each(context, EditKind::Deleting, |selection| {
        if !selection.is_empty() {
            return Some(Replacement::new(selection.start(), selection.end(), ""));
        }
        let head = selection.head;
        let position = text.position(head);
        if position.column == 0 {
            return (position.line > 0).then(|| Replacement::new(text.line_content_end(position.line - 1), head, ""));
        }
        let line = text.line_content(position.line);
        let line_start = text.line_start(position.line);
        let start_column = match unit {
            DeleteUnit::Char => {
                if let Some(pair_end) = auto_closed_pair_end(context, head) {
                    return Some(Replacement::new(head - 1, pair_end, ""));
                }
                indentation_delete_start(context.config, &line, position.column)
                    .unwrap_or_else(|| previous_grapheme_column(&line, position.column))
            }
            DeleteUnit::Word => {
                let chars: Vec<char> = line.chars().collect();
                context.config.words.previous_word_start(&chars, position.column).unwrap_or(0)
            }
            DeleteUnit::WordPart => {
                let chars: Vec<char> = line.chars().collect();
                crate::words::previous_word_part_start(&chars, position.column).unwrap_or(0)
            }
            DeleteUnit::Line => 0,
        };
        Some(Replacement::new(line_start + start_column, head, ""))
    })
}

/// Delete and its variants.
pub fn delete_forward(context: &EditContext, unit: DeleteUnit) -> Option<Transaction> {
    let text = context.text;
    replace_each(context, EditKind::Deleting, |selection| {
        if !selection.is_empty() {
            return Some(Replacement::new(selection.start(), selection.end(), ""));
        }
        let head = selection.head;
        let position = text.position(head);
        if position.column == text.line_len(position.line) {
            return (position.line < text.last_line())
                .then(|| Replacement::new(head, text.line_start(position.line + 1), ""));
        }
        let line = text.line_content(position.line);
        let line_start = text.line_start(position.line);
        let chars: Vec<char> = line.chars().collect();
        let end_column = match unit {
            DeleteUnit::Char => next_grapheme_column(&line, position.column),
            DeleteUnit::Word => context.config.words.next_word_end(&chars, position.column).unwrap_or(chars.len()),
            DeleteUnit::WordPart => crate::words::next_word_part_start(&chars, position.column).unwrap_or(chars.len()),
            DeleteUnit::Line => chars.len(),
        };
        Some(Replacement::new(head, line_start + end_column, ""))
    })
}

/// Swaps the chars around each cursor and moves past them: `t|eh` → `et|h`, and at a line end
/// the two before it, `teh|` → `the|`. Selections and cursors at a line start are left alone
/// (VS Code's `transposeLetters`).
pub fn transpose_letters(context: &EditContext) -> Option<Transaction> {
    let text = context.text;
    replace_each(context, EditKind::Other, |selection| {
        if !selection.is_empty() {
            return None;
        }
        let position = text.position(selection.head);
        let chars: Vec<char> = text.line_content(position.line).chars().collect();
        let end_column = if position.column == chars.len() { position.column } else { position.column + 1 };
        let begin_column = end_column.checked_sub(2)?;
        let line_start = text.line_start(position.line);
        let swapped: String = [chars[begin_column + 1], chars[begin_column]].iter().collect();
        Some(Replacement::new(line_start + begin_column, line_start + end_column, swapped))
    })
}

/// When the cursor is right after an auto-closed opening char, with its closing char right
/// after the cursor: the offset after that closing char.
fn auto_closed_pair_end(context: &EditContext, head: usize) -> Option<usize> {
    if !context.config.auto_closing_delete || !context.auto_closed_offsets.contains(&head) {
        return None;
    }
    let previous_char = context.text.char_at(head.checked_sub(1)?)?;
    let next_char = context.text.char_at(head)?;
    let is_pair =
        context.language.auto_closing_pairs.iter().any(|pair| pair.open == previous_char && pair.close == next_char);
    is_pair.then_some(head + 1)
}

/// In leading spaces, backspace goes back to the previous tab stop.
fn indentation_delete_start(config: &EditingConfig, line: &str, column: usize) -> Option<usize> {
    let before_cursor: String = line.chars().take(column).collect();
    let is_in_leading_spaces = !before_cursor.is_empty() && before_cursor.chars().all(|character| character == ' ');
    if !config.use_tab_stops || !config.indentation.insert_spaces || !is_in_leading_spaces {
        return None;
    }
    Some(config.indentation.previous_tab_stop(column))
}

/// Enter: a line break in the text's line ending, keeping the indentation, indenting after an
/// opening bracket and putting its closing bracket on a line of its own.
pub fn insert_newline(context: &EditContext) -> Option<Transaction> {
    let (text, config, language) = (context.text, context.config, context.language);
    let line_ending = text.line_ending().as_str();
    replace_each(context, EditKind::Other, |selection| {
        let (start, end) = (selection.start(), selection.end());
        let position = text.position(start);
        let line = text.line_content(position.line);
        let line_before_cursor: String = line.chars().take(position.column).collect();
        if !config.auto_indent {
            return Some(Replacement::new(start, end, line_ending));
        }

        let indent = leading_whitespace(&line_before_cursor).to_string();
        let char_before = line_before_cursor.trim_end().chars().last();
        let char_after = text.char_at(end);
        let closing_bracket = char_before.and_then(|open| language.closing_bracket_of(open));
        let Some(closing_bracket) = closing_bracket else {
            return Some(Replacement::new(start, end, format!("{line_ending}{indent}")));
        };

        let inner_indent = format!("{indent}{}", config.indentation.unit());
        let first_part = format!("{line_ending}{inner_indent}");
        let cursor = first_part.chars().count();
        if char_after == Some(closing_bracket) {
            let insert = format!("{first_part}{line_ending}{indent}");
            return Some(Replacement::new(start, end, insert).with_cursor(cursor));
        }
        Some(Replacement::new(start, end, first_part))
    })
}

/// Tab: indents the lines when a selection spans several lines, else inserts whitespace up to
/// the next tab stop (or a tab char).
pub fn insert_tab(context: &EditContext) -> Option<Transaction> {
    let text = context.text;
    let spans_lines = context.selections.iter().any(|selection| !selection.range(text).is_single_line());
    if spans_lines {
        return indent_lines(context);
    }

    let indentation = context.config.indentation;
    replace_each(context, EditKind::Typing, |selection| {
        if !indentation.insert_spaces {
            return Some(Replacement::new(selection.start(), selection.end(), "\t"));
        }
        let position = text.position(selection.start());
        let visual_column = indentation.visual_column(&text.line_content(position.line), position.column);
        let spaces = indentation.tab_size - visual_column % indentation.tab_size;
        Some(Replacement::new(selection.start(), selection.end(), " ".repeat(spaces)))
    })
}

/// Lines covered by any selection, each once, in order.
pub fn covered_lines(text: &Text, selections: &Selections) -> Vec<usize> {
    let mut lines: Vec<usize> = selections
        .iter()
        .flat_map(|selection| {
            let (first_line, last_line) = selection.covered_lines(text);
            first_line..=last_line
        })
        .collect();
    lines.dedup();
    lines
}

/// One indent unit at the start of each covered non-empty line.
pub fn indent_lines(context: &EditContext) -> Option<Transaction> {
    let text = context.text;
    let unit = context.config.indentation.unit();
    let is_single_line = covered_lines(text, context.selections).len() == 1;
    let changes = covered_lines(text, context.selections)
        .into_iter()
        .filter(|&line| is_single_line || text.line_len(line) > 0)
        .map(|line| Change::insert(text.line_start(line), unit.clone()));
    line_start_transaction(context, ChangeSet::new(changes))
}

/// Removes up to one indent unit from the start of each covered line.
pub fn outdent_lines(context: &EditContext) -> Option<Transaction> {
    let text = context.text;
    let tab_size = context.config.indentation.tab_size;
    let changes = covered_lines(text, context.selections).into_iter().filter_map(|line| {
        let content = text.line_content(line);
        let removed_len = if content.starts_with('\t') {
            1
        } else {
            content.chars().take(tab_size).take_while(|&character| character == ' ').count()
        };
        let line_start = text.line_start(line);
        (removed_len > 0).then(|| Change::delete(line_start, line_start + removed_len))
    });
    line_start_transaction(context, ChangeSet::new(changes))
}

/// A transaction of changes at line starts: selection ends at a line start stay at the start.
fn line_start_transaction(context: &EditContext, changes: ChangeSet) -> Option<Transaction> {
    if changes.is_empty() {
        return None;
    }
    let selections = context.selections.map_each(|selection| {
        Selection::new(
            changes.map_offset(selection.anchor, Bias::Before),
            changes.map_offset(selection.head, Bias::Before),
        )
    });
    Some(Transaction { changes, selections, kind: EditKind::Other, auto_closed_offsets: Vec::new() })
}

/// What a copy or cut puts on the clipboard. `is_whole_lines` marks a copy of whole lines made
/// with empty selections: pasting it with an empty selection inserts it above the cursor's line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardContent {
    pub text: String,
    pub is_whole_lines: bool,
}

/// The text a copy takes: the selections joined with line breaks, or the cursors' whole lines.
pub fn copied_content(context: &EditContext) -> Option<ClipboardContent> {
    let text = context.text;
    let line_ending = text.line_ending().as_str();
    let all_empty = context.selections.iter().all(Selection::is_empty);
    if all_empty && !context.config.empty_selection_clipboard {
        return None;
    }
    if all_empty {
        let lines = covered_lines(text, context.selections)
            .into_iter()
            .map(|line| text.line_content(line) + line_ending)
            .collect();
        return Some(ClipboardContent { text: lines, is_whole_lines: true });
    }
    let parts: Vec<String> = context
        .selections
        .iter()
        .filter(|selection| !selection.is_empty())
        .map(|selection| text.slice(selection.start(), selection.end()))
        .collect();
    Some(ClipboardContent { text: parts.join(line_ending), is_whole_lines: false })
}

/// Cut: deletes what `copied_content` takes (whole lines for empty selections).
pub fn cut(context: &EditContext) -> Option<Transaction> {
    let text = context.text;
    let all_empty = context.selections.iter().all(Selection::is_empty);
    if !all_empty {
        return replace_each(context, EditKind::Other, |selection| {
            Some(Replacement::new(selection.start(), selection.end(), ""))
        });
    }
    if !context.config.empty_selection_clipboard {
        return None;
    }
    let changes = covered_lines(text, context.selections).into_iter().map(|line| {
        let (start, end) = whole_line_offsets(text, line);
        Change::delete(start, end)
    });
    let changes = ChangeSet::new(changes);
    let selections = context.selections.map(&changes);
    Some(Transaction { changes, selections, kind: EditKind::Other, auto_closed_offsets: Vec::new() })
}

/// A line with its line ending; the last line takes the line ending before it instead.
pub fn whole_line_offsets(text: &Text, line: usize) -> (usize, usize) {
    if line < text.last_line() {
        return (text.line_start(line), text.line_start(line + 1));
    }
    let start = if line > 0 { text.line_content_end(line - 1) } else { 0 };
    (start, text.len_chars())
}

/// Paste: spreads one line per cursor when the counts match, inserts whole lines above the
/// cursor's line when they were copied that way, else replaces each selection.
pub fn paste(context: &EditContext, content: &ClipboardContent) -> Option<Transaction> {
    use crate::config::MultiCursorPaste;
    let text = context.text;

    let pasted_lines: Vec<&str> =
        content.text.trim_end_matches(['\r', '\n']).split('\n').map(|line| line.trim_end_matches('\r')).collect();
    let spreads = context.selections.is_multiple()
        && context.config.multi_cursor_paste == MultiCursorPaste::Spread
        && pasted_lines.len() == context.selections.len();
    if spreads {
        let mut lines = pasted_lines.iter();
        return replace_each(context, EditKind::Other, |selection| {
            Some(Replacement::new(selection.start(), selection.end(), lines.next().copied().unwrap_or_default()))
        });
    }

    replace_each(context, EditKind::Other, |selection| {
        if content.is_whole_lines && selection.is_empty() {
            let position = text.position(selection.head);
            let line_start = text.line_start(position.line);
            let mut inserted = content.text.clone();
            if !inserted.ends_with('\n') {
                inserted.push_str(text.line_ending().as_str());
            }
            let inserted_len = inserted.chars().count();
            return Some(
                Replacement::new(line_start, line_start, inserted).with_cursor(inserted_len + position.column),
            );
        }
        Some(Replacement::new(selection.start(), selection.end(), content.text.clone()))
    })
}

#[cfg(test)]
mod tests {
    use crate::selection::Selections;
    use crate::state::{Command, EditorState};

    fn transposed(content: &str, cursor: usize) -> (String, usize) {
        let mut state = EditorState::new(content);
        state.execute(Command::SetSelections(Selections::cursor(cursor)));
        state.execute(Command::TransposeLetters);
        (state.text().to_string(), state.selections().primary().head)
    }

    #[test]
    fn transposing_swaps_the_chars_around_the_cursor_and_moves_past_them() {
        assert_eq!(transposed("teh", 1), ("eth".to_string(), 2));
    }

    #[test]
    fn transposing_at_a_line_end_swaps_the_two_chars_before() {
        assert_eq!(transposed("teh\nx", 3), ("the\nx".to_string(), 3));
    }

    #[test]
    fn transposing_at_a_line_start_does_nothing() {
        assert_eq!(transposed("ab\ncd", 3), ("ab\ncd".to_string(), 3));
    }
}
