//! Typing in a tag name types in its pair too (`EditingConfig::linked_editing`, DECISIONS D34):
//! the command runs with a second cursor mirrored in the other name, in one undo step, and only
//! the user's cursor is left afterwards.

use super::{Command, EditorState, Outcome};
use crate::edit::{self, DeleteUnit, EditContext};
use crate::linked_editing::{NameRange, is_tag_name_char, tag_pair_names};
use crate::selection::{Selection, Selections};

impl EditorState {
    /// Runs `command` in both names of a tag pair, if it edits the name the single cursor is in.
    pub(super) fn execute_linked(&mut self, command: &Command) -> Option<Outcome> {
        if self.selections.is_multiple() || !edits_a_name(command) {
            return None;
        }
        let primary = self.selections.primary();
        let [own, other] = tag_pair_names(&self.text, primary.head)?;
        let mirror = mirrored(primary, own, other)?;
        if !stays_in_the_name(command, primary, own) {
            return None;
        }

        let both = Selections::new(vec![primary, mirror], 0);
        let context = EditContext { selections: &both, ..self.context() };
        let mut transaction = match command {
            Command::Type(typed) => edit::type_text(&context, typed),
            Command::DeleteBackward(unit) => edit::delete_backward(&context, *unit),
            Command::DeleteForward(unit) => edit::delete_forward(&context, *unit),
            _ => None,
        }?;
        let user_cursor = transaction.selections.as_slice()[usize::from(own.0 > other.0)];
        transaction.selections = Selections::single(user_cursor);
        Some(self.apply(transaction))
    }
}

/// Typing name chars, or deleting a char.
fn edits_a_name(command: &Command) -> bool {
    match command {
        Command::Type(typed) => !typed.is_empty() && typed.chars().all(is_tag_name_char),
        Command::DeleteBackward(unit) | Command::DeleteForward(unit) => *unit == DeleteUnit::Char,
        _ => false,
    }
}

/// Whether a delete stays inside the name: backspace at its start would eat the `<`.
fn stays_in_the_name(command: &Command, selection: Selection, name: NameRange) -> bool {
    match command {
        Command::DeleteBackward(_) => !selection.is_empty() || selection.head > name.0,
        Command::DeleteForward(_) => !selection.is_empty() || selection.head < name.1,
        _ => true,
    }
}

/// `selection` moved from the name `own` to the same place in `other`, if it lies in `own`.
fn mirrored(selection: Selection, own: NameRange, other: NameRange) -> Option<Selection> {
    let is_inside = own.0 <= selection.start() && selection.end() <= own.1;
    let shift = |offset: usize| other.0 + offset - own.0;
    is_inside.then(|| Selection::new(shift(selection.anchor), shift(selection.head)))
}

#[cfg(test)]
mod tests {
    use crate::config::EditingConfig;
    use crate::edit::DeleteUnit;
    use crate::selection::Selections;
    use crate::state::{Command, EditorState};

    fn linked(content: &str, cursor: usize) -> EditorState {
        let mut state =
            EditorState::new(content).with_config(EditingConfig { linked_editing: true, ..EditingConfig::default() });
        state.execute(Command::SetSelections(Selections::cursor(cursor)));
        state
    }

    #[test]
    fn typing_in_an_open_tag_name_renames_its_close_tag() {
        let mut state = linked("<div>x</div>", 4);

        state.execute(Command::Type("s".into()));

        assert_eq!(state.text().to_string(), "<divs>x</divs>");
        assert_eq!(state.selections(), &Selections::cursor(5));
    }

    #[test]
    fn typing_in_a_close_tag_name_renames_its_open_tag() {
        let mut state = linked("<p>x</p>", 7);

        state.execute(Command::DeleteBackward(DeleteUnit::Char));
        state.execute(Command::Type("em".into()));

        assert_eq!(state.text().to_string(), "<em>x</em>");
        assert_eq!(state.selections(), &Selections::cursor(9));
    }

    #[test]
    fn a_rename_is_one_undo_step() {
        let mut state = linked("<a>x</a>", 2);
        state.execute(Command::Type("b".into()));
        state.execute(Command::Type("c".into()));

        state.execute(Command::Undo);

        assert_eq!(state.text().to_string(), "<a>x</a>");
        assert!(!state.selections().is_multiple());
    }

    #[test]
    fn a_space_ends_the_name_and_types_once() {
        let mut state = linked("<div>x</div>", 4);

        state.execute(Command::Type(" ".into()));

        assert_eq!(state.text().to_string(), "<div >x</div>");
    }

    #[test]
    fn backspace_at_a_name_start_is_not_mirrored() {
        let mut state = linked("<div>x</div>", 1);

        state.execute(Command::DeleteBackward(DeleteUnit::Char));

        assert_eq!(state.text().to_string(), "div>x</div>");
    }

    #[test]
    fn without_the_option_only_one_name_changes() {
        let mut state = EditorState::new("<div>x</div>");
        state.execute(Command::SetSelections(Selections::cursor(4)));

        state.execute(Command::Type("s".into()));

        assert_eq!(state.text().to_string(), "<divs>x</div>");
    }
}
