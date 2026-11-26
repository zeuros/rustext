//! The editor state: text, selections, history and configuration, changed only by commands
//! (D7, D8). Every command is computed by a pure function; `execute` applies its result.

use crate::change::ChangeSet;
use crate::comment::{toggle_block_comment, toggle_line_comment};
use crate::config::{EditingConfig, LanguageConfiguration};
use crate::cursors::{self, VerticalDirection};
use crate::edit::{self, ClipboardContent, DeleteUnit, EditContext, Transaction};
use crate::history::History;
use crate::lines::{self, CaseTransform, LinesRewrite};
use crate::motion::{Motion, apply_motion};
use crate::position::Position;
use crate::selection::Selections;
use crate::text::Text;

/// Everything the editor can do to its state. Commands that need the viewport (page moves)
/// carry what they need.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Move {
        motion: Motion,
        extend: bool,
    },
    SetSelections(Selections),
    SelectAll,
    /// Ctrl+L.
    ExpandToLines,
    /// Escape.
    CollapseSelections,
    AddCursor(VerticalDirection),
    /// Ctrl+D.
    AddNextOccurrence,
    /// Ctrl+Shift+L.
    SelectAllOccurrences,
    /// Shift+Alt+I.
    CursorsAtLineEnds,
    /// Shift+Alt+drag.
    ColumnSelect {
        anchor: Position,
        head_line: usize,
        head_visual_column: usize,
    },

    Type(String),
    Newline,
    Tab,
    Indent,
    Outdent,
    DeleteBackward(DeleteUnit),
    DeleteForward(DeleteUnit),
    Cut,
    Copy,
    Paste(String),
    Undo,
    Redo,

    MoveLines(VerticalDirection),
    CopyLines(VerticalDirection),
    DeleteLines,
    DuplicateSelection,
    JoinLines,
    RewriteLines(LinesRewrite),
    TrimTrailingWhitespace,
    InsertLine(VerticalDirection),
    TransformCase(CaseTransform),
    ToggleLineComment,
    ToggleBlockComment,
    /// Applies a transaction computed outside (find / replace, formatting, an application edit).
    Apply(Transaction),
}

impl Command {
    /// Whether the command changes the text (rejected when read-only).
    pub fn edits_text(&self) -> bool {
        !matches!(
            self,
            Command::Move { .. }
                | Command::SetSelections(_)
                | Command::SelectAll
                | Command::ExpandToLines
                | Command::CollapseSelections
                | Command::AddCursor(_)
                | Command::AddNextOccurrence
                | Command::SelectAllOccurrences
                | Command::CursorsAtLineEnds
                | Command::ColumnSelect { .. }
                | Command::Copy
        )
    }

    /// A stable name for keymaps in settings files and the command palette.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Move { .. } => "cursor.move",
            Command::SetSelections(_) => "cursor.setSelections",
            Command::SelectAll => "selection.selectAll",
            Command::ExpandToLines => "selection.expandToLines",
            Command::CollapseSelections => "selection.collapse",
            Command::AddCursor(VerticalDirection::Up) => "cursor.addAbove",
            Command::AddCursor(VerticalDirection::Down) => "cursor.addBelow",
            Command::AddNextOccurrence => "selection.addNextOccurrence",
            Command::SelectAllOccurrences => "selection.selectAllOccurrences",
            Command::CursorsAtLineEnds => "cursor.atLineEnds",
            Command::ColumnSelect { .. } => "selection.column",
            Command::Type(_) => "edit.type",
            Command::Newline => "edit.newline",
            Command::Tab => "edit.tab",
            Command::Indent => "edit.indent",
            Command::Outdent => "edit.outdent",
            Command::DeleteBackward(_) => "edit.deleteBackward",
            Command::DeleteForward(_) => "edit.deleteForward",
            Command::Cut => "clipboard.cut",
            Command::Copy => "clipboard.copy",
            Command::Paste(_) => "clipboard.paste",
            Command::Undo => "history.undo",
            Command::Redo => "history.redo",
            Command::MoveLines(VerticalDirection::Up) => "lines.moveUp",
            Command::MoveLines(VerticalDirection::Down) => "lines.moveDown",
            Command::CopyLines(VerticalDirection::Up) => "lines.copyUp",
            Command::CopyLines(VerticalDirection::Down) => "lines.copyDown",
            Command::DeleteLines => "lines.delete",
            Command::DuplicateSelection => "selection.duplicate",
            Command::JoinLines => "lines.join",
            Command::RewriteLines(LinesRewrite::SortAscending) => "lines.sortAscending",
            Command::RewriteLines(LinesRewrite::SortDescending) => "lines.sortDescending",
            Command::RewriteLines(LinesRewrite::Reverse) => "lines.reverse",
            Command::RewriteLines(LinesRewrite::RemoveDuplicates) => "lines.removeDuplicates",
            Command::TrimTrailingWhitespace => "lines.trimTrailingWhitespace",
            Command::InsertLine(VerticalDirection::Up) => "lines.insertAbove",
            Command::InsertLine(VerticalDirection::Down) => "lines.insertBelow",
            Command::TransformCase(CaseTransform::Upper) => "case.upper",
            Command::TransformCase(CaseTransform::Lower) => "case.lower",
            Command::TransformCase(CaseTransform::Title) => "case.title",
            Command::TransformCase(CaseTransform::Snake) => "case.snake",
            Command::TransformCase(CaseTransform::Camel) => "case.camel",
            Command::TransformCase(CaseTransform::Pascal) => "case.pascal",
            Command::TransformCase(CaseTransform::Kebab) => "case.kebab",
            Command::ToggleLineComment => "comment.line",
            Command::ToggleBlockComment => "comment.block",
            Command::Apply(_) => "edit.apply",
        }
    }
}

/// What executing a command did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// The change sets applied to the text, in order (several for an undo of grouped typing).
    pub applied_changes: Vec<ChangeSet>,
    pub selections_changed: bool,
    /// An editing command was rejected because the editor is read-only.
    pub rejected_read_only: bool,
    /// What copy or cut put on the clipboard.
    pub clipboard: Option<ClipboardContent>,
}

impl Outcome {
    pub fn text_changed(&self) -> bool {
        !self.applied_changes.is_empty()
    }

    /// Merges what a later command did into this outcome.
    pub fn merged(mut self, later: Outcome) -> Outcome {
        self.applied_changes.extend(later.applied_changes);
        self.selections_changed |= later.selections_changed;
        self.rejected_read_only |= later.rejected_read_only;
        self.clipboard = later.clipboard.or(self.clipboard);
        self
    }
}

#[derive(Clone, Debug)]
pub struct EditorState {
    text: Text,
    selections: Selections,
    history: History,
    pub config: EditingConfig,
    pub language: LanguageConfiguration,
    /// Closing chars the editor inserted after their opening char, which typing steps over.
    auto_closed_offsets: Vec<usize>,
    /// The last copy, to know whether a paste of the same text is whole lines.
    last_copy: Option<ClipboardContent>,
}

impl EditorState {
    pub fn new(content: &str) -> Self {
        Self::with_text(Text::new(content))
    }

    pub fn with_text(text: Text) -> Self {
        Self {
            text,
            selections: Selections::default(),
            history: History::default(),
            config: EditingConfig::default(),
            language: LanguageConfiguration::default(),
            auto_closed_offsets: Vec::new(),
            last_copy: None,
        }
    }

    pub fn with_config(self, config: EditingConfig) -> Self {
        Self { config, ..self }
    }

    pub fn with_language(self, language: LanguageConfiguration) -> Self {
        Self { language, ..self }
    }

    pub fn text(&self) -> &Text {
        &self.text
    }

    pub fn selections(&self) -> &Selections {
        &self.selections
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    /// Replaces the whole text (a file reloaded from disk): new history, cursor at the start.
    pub fn set_text(&mut self, text: Text) {
        self.text = text;
        self.selections = Selections::default();
        self.history = History::default();
        self.auto_closed_offsets.clear();
    }

    pub fn context(&self) -> EditContext<'_> {
        EditContext {
            text: &self.text,
            selections: &self.selections,
            config: &self.config,
            language: &self.language,
            auto_closed_offsets: &self.auto_closed_offsets,
        }
    }

    /// Marks a boundary in history: the next typing starts a new undo step.
    pub fn push_undo_stop(&mut self) {
        self.history.push_undo_stop();
    }

    pub fn execute(&mut self, command: Command) -> Outcome {
        if command.edits_text() && self.config.read_only {
            let clipboard = matches!(command, Command::Cut).then(|| edit::copied_content(&self.context())).flatten();
            return Outcome { rejected_read_only: true, clipboard, ..Outcome::default() };
        }

        let context = self.context();
        let (text, config) = (&self.text, &self.config);
        let selections = &self.selections;
        let transaction = match command {
            Command::Move { motion, extend } => {
                let moved = selections.map_each(|selection| {
                    let moved = apply_motion(text, config, selection, motion, extend);
                    if motion.is_vertical() { moved } else { moved.with_preferred_visual_column(None) }
                });
                return self.select(moved);
            }
            Command::SetSelections(new_selections) => return self.select(new_selections.clamped(text)),
            Command::SelectAll => return self.select(cursors::select_all(text)),
            Command::ExpandToLines => return self.select(cursors::expand_to_lines(text, selections)),
            Command::CollapseSelections => return self.select(cursors::collapse(selections)),
            Command::AddCursor(direction) => {
                return self.select(cursors::add_cursors_vertically(text, config, selections, direction));
            }
            Command::AddNextOccurrence => return self.select(cursors::add_next_occurrence(text, config, selections)),
            Command::SelectAllOccurrences => {
                return self.select(cursors::select_all_occurrences(text, config, selections));
            }
            Command::CursorsAtLineEnds => return self.select(cursors::cursors_at_line_ends(text, selections)),
            Command::ColumnSelect { anchor, head_line, head_visual_column } => {
                return self.select(cursors::column_selection(text, config, anchor, head_line, head_visual_column));
            }
            Command::Copy => {
                let clipboard = edit::copied_content(&context);
                self.last_copy = clipboard.clone();
                return Outcome { clipboard, ..Outcome::default() };
            }
            Command::Undo => return self.replay(true),
            Command::Redo => return self.replay(false),

            Command::Type(typed) => edit::type_text(&context, &typed),
            Command::Newline => edit::insert_newline(&context),
            Command::Tab => edit::insert_tab(&context),
            Command::Indent => edit::indent_lines(&context),
            Command::Outdent => edit::outdent_lines(&context),
            Command::DeleteBackward(unit) => edit::delete_backward(&context, unit),
            Command::DeleteForward(unit) => edit::delete_forward(&context, unit),
            Command::Cut => {
                let clipboard = edit::copied_content(&context);
                let transaction = edit::cut(&context);
                self.last_copy = clipboard.clone();
                let outcome = transaction.map(|transaction| self.apply(transaction)).unwrap_or_default();
                return Outcome { clipboard, ..outcome };
            }
            Command::Paste(pasted) => {
                let is_whole_lines =
                    self.last_copy.as_ref().is_some_and(|copy| copy.is_whole_lines && copy.text == pasted);
                edit::paste(&context, &ClipboardContent { text: pasted, is_whole_lines })
            }
            Command::MoveLines(direction) => lines::move_lines(&context, direction),
            Command::CopyLines(direction) => lines::copy_lines(&context, direction),
            Command::DeleteLines => lines::delete_lines(&context),
            Command::DuplicateSelection => lines::duplicate_selection(&context),
            Command::JoinLines => lines::join_lines(&context),
            Command::RewriteLines(rewrite) => lines::rewrite_lines(&context, rewrite),
            Command::TrimTrailingWhitespace => lines::trim_trailing_whitespace(&context),
            Command::InsertLine(direction) => lines::insert_line(&context, direction),
            Command::TransformCase(transform) => lines::transform_case(&context, transform),
            Command::ToggleLineComment => toggle_line_comment(&context),
            Command::ToggleBlockComment => toggle_block_comment(&context),
            Command::Apply(transaction) => Some(transaction),
        };
        transaction.map(|transaction| self.apply(transaction)).unwrap_or_default()
    }

    /// Runs several commands, merging their outcomes.
    pub fn execute_all(&mut self, commands: impl IntoIterator<Item = Command>) -> Outcome {
        commands.into_iter().fold(Outcome::default(), |outcome, command| outcome.merged(self.execute(command)))
    }

    /// Applies a transaction and records it in history.
    pub fn apply(&mut self, transaction: Transaction) -> Outcome {
        if transaction.changes.is_empty() {
            return self.select(transaction.selections);
        }
        let new_text = transaction.changes.apply(&self.text);
        let selections_after = transaction.selections.clamped(&new_text);
        self.history.record(
            &self.text,
            transaction.changes.clone(),
            self.selections.clone(),
            selections_after.clone(),
            transaction.kind,
        );

        let mut auto_closed_offsets: Vec<usize> = self
            .auto_closed_offsets
            .iter()
            .map(|&offset| transaction.changes.map_offset(offset, crate::change::Bias::After))
            .chain(transaction.auto_closed_offsets)
            .collect();
        auto_closed_offsets.sort_unstable();
        auto_closed_offsets.dedup();

        let selections_changed = selections_after != self.selections;
        self.text = new_text;
        self.selections = selections_after;
        self.auto_closed_offsets = auto_closed_offsets;
        self.forget_auto_closed_away_from_cursors();
        Outcome { applied_changes: vec![transaction.changes], selections_changed, ..Outcome::default() }
    }

    fn select(&mut self, selections: Selections) -> Outcome {
        if selections == self.selections {
            return Outcome::default();
        }
        self.selections = selections;
        self.history.push_undo_stop();
        self.forget_auto_closed_away_from_cursors();
        Outcome { selections_changed: true, ..Outcome::default() }
    }

    fn replay(&mut self, is_undo: bool) -> Outcome {
        let replay = if is_undo { self.history.undo() } else { self.history.redo() };
        let Some(replay) = replay else {
            return Outcome::default();
        };
        for changes in &replay.changes {
            self.text = changes.apply(&self.text);
        }
        self.selections = replay.selections.clamped(&self.text);
        self.auto_closed_offsets.clear();
        Outcome { applied_changes: replay.changes, selections_changed: true, ..Outcome::default() }
    }

    /// Auto-closed chars only stay "typeable over" while a cursor is on their line.
    fn forget_auto_closed_away_from_cursors(&mut self) {
        let text = &self.text;
        let cursor_lines: Vec<usize> =
            self.selections.iter().map(|selection| text.line_of_offset(selection.head)).collect();
        self.auto_closed_offsets.retain(|&offset| cursor_lines.contains(&text.line_of_offset(offset)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LanguageConfiguration;
    use crate::selection::Selection;

    fn editor(content: &str, cursor: usize) -> EditorState {
        let mut state = EditorState::new(content).with_language(LanguageConfiguration::c_like());
        state.execute(Command::SetSelections(Selections::cursor(cursor)));
        state
    }

    fn type_all(state: &mut EditorState, typed: &str) {
        for character in typed.chars() {
            state.execute(Command::Type(character.to_string()));
        }
    }

    #[test]
    fn typing_auto_closes_and_steps_over_the_closing_bracket() {
        let mut state = editor("", 0);

        type_all(&mut state, "f(x)");

        assert_eq!(state.text().to_string(), "f(x)");
        assert_eq!(state.selections().primary(), Selection::cursor(4));
    }

    #[test]
    fn closing_bracket_typed_before_an_existing_one_is_inserted() {
        let mut state = editor(")", 0);

        type_all(&mut state, ")");

        assert_eq!(state.text().to_string(), "))");
    }

    #[test]
    fn backspace_between_an_auto_closed_pair_deletes_both() {
        let mut state = editor("", 0);
        type_all(&mut state, "[");

        state.execute(Command::DeleteBackward(DeleteUnit::Char));

        assert_eq!(state.text().to_string(), "");
    }

    #[test]
    fn no_auto_close_before_a_word_nor_a_quote_inside_a_word() {
        let mut state = editor("x", 0);
        type_all(&mut state, "(");
        assert_eq!(state.text().to_string(), "(x");

        let mut state = editor("don", 3);
        type_all(&mut state, "'");
        assert_eq!(state.text().to_string(), "don'");
    }

    #[test]
    fn typing_a_bracket_surrounds_the_selection() {
        let mut state = editor("abc", 0);
        state.execute(Command::SetSelections(Selections::single(Selection::new(0, 3))));

        type_all(&mut state, "(");

        assert_eq!(state.text().to_string(), "(abc)");
        assert_eq!(state.selections().primary(), Selection::new(1, 4));
    }

    #[test]
    fn enter_between_brackets_indents_and_moves_the_closing_one_down() {
        let mut state = editor("  f {}", 5);

        state.execute(Command::Newline);

        assert_eq!(state.text().to_string(), "  f {\n      \n  }");
        assert_eq!(state.text().position(state.selections().primary().head), Position::new(1, 6));
    }

    #[test]
    fn enter_keeps_crlf_files_crlf() {
        let mut state = editor("a\r\nb", 1);

        state.execute(Command::Newline);

        assert_eq!(state.text().to_string(), "a\r\n\r\nb");
    }

    #[test]
    fn backspace_in_indentation_goes_back_a_tab_stop() {
        let mut state = editor("      x", 6);

        state.execute(Command::DeleteBackward(DeleteUnit::Char));

        assert_eq!(state.text().to_string(), "    x");
    }

    #[test]
    fn backspace_at_line_start_joins_crlf_lines() {
        let mut state = editor("a\r\nb", 3);

        state.execute(Command::DeleteBackward(DeleteUnit::Char));

        assert_eq!(state.text().to_string(), "ab");
    }

    #[test]
    fn typing_at_several_cursors() {
        let mut state = editor("a\nb\nc", 0);
        state.execute(Command::AddCursor(VerticalDirection::Down));
        state.execute(Command::AddCursor(VerticalDirection::Down));

        type_all(&mut state, "> ");

        assert_eq!(state.text().to_string(), "> a\n> b\n> c");
        assert_eq!(state.selections().len(), 3);
    }

    #[test]
    fn undo_restores_text_and_cursors_redo_replays() {
        let mut state = editor("", 0);
        type_all(&mut state, "hello");
        state.execute(Command::Move { motion: Motion::Left, extend: false });
        type_all(&mut state, "X");

        state.execute(Command::Undo);
        assert_eq!(state.text().to_string(), "hello");
        assert_eq!(state.selections().primary(), Selection::cursor(4));
        state.execute(Command::Undo);
        assert_eq!(state.text().to_string(), "");
        state.execute(Command::Redo);
        assert_eq!(state.text().to_string(), "hello");
    }

    #[test]
    fn copy_of_an_empty_selection_takes_the_line_and_pastes_above() {
        let mut state = editor("one\ntwo", 5);

        let clipboard = state.execute(Command::Copy).clipboard.expect("copied");
        assert_eq!(clipboard.text, "two\n");
        state.execute(Command::Paste(clipboard.text));

        assert_eq!(state.text().to_string(), "one\ntwo\ntwo");
        assert_eq!(state.text().position(state.selections().primary().head), Position::new(2, 1));
    }

    #[test]
    fn paste_spreads_lines_over_cursors() {
        let mut state = editor("a\nb", 1);
        state.execute(Command::CursorsAtLineEnds);
        state.execute(Command::SelectAll);
        state.execute(Command::CursorsAtLineEnds);

        state.execute(Command::Paste("1\n2".into()));

        assert_eq!(state.text().to_string(), "a1\nb2");
    }

    #[test]
    fn read_only_rejects_edits_but_copies() {
        let mut state = editor("abc", 0).with_config(EditingConfig { read_only: true, ..EditingConfig::default() });

        let outcome = state.execute(Command::Type("x".into()));
        assert!(outcome.rejected_read_only);
        assert_eq!(state.text().to_string(), "abc");
        assert!(state.execute(Command::Copy).clipboard.is_some());
    }

    #[test]
    fn tab_inserts_spaces_to_the_next_tab_stop() {
        let mut state = editor("ab", 2);

        state.execute(Command::Tab);

        assert_eq!(state.text().to_string(), "ab  ");
    }

    #[test]
    fn tab_with_a_multi_line_selection_indents_and_shift_tab_outdents() {
        let mut state = editor("a\nb", 0);
        state.execute(Command::SelectAll);

        state.execute(Command::Tab);
        assert_eq!(state.text().to_string(), "    a\n    b");
        assert_eq!(state.selections().primary(), Selection::new(0, 11));

        state.execute(Command::Outdent);
        assert_eq!(state.text().to_string(), "a\nb");
    }

    #[test]
    fn dirty_tracking_with_history_versions() {
        let mut state = editor("", 0);
        let saved_version = state.history().version();

        type_all(&mut state, "x");
        assert_ne!(state.history().version(), saved_version);
        state.execute(Command::Undo);
        assert_eq!(state.history().version(), saved_version);
    }
}
