//! Cursor undo and redo (VS Code's `cursorUndo`, Ctrl+U): back to the selections before the
//! last cursor move, without touching the text. An edit forgets them, as in VS Code: offsets
//! from before it would point elsewhere.

use super::{EditorState, Outcome};
use crate::selection::Selections;

/// Selections kept for cursor undo; older ones are dropped.
const MAX_CURSOR_STATES: usize = 50;

/// The selections before each cursor move, and those cursor undo went back from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct CursorHistory {
    undo: Vec<Selections>,
    redo: Vec<Selections>,
}

impl CursorHistory {
    /// The selections were `before` until a cursor move.
    pub(super) fn record(&mut self, before: Selections) {
        if self.undo.len() == MAX_CURSOR_STATES {
            self.undo.remove(0);
        }
        self.undo.push(before);
        self.redo.clear();
    }

    pub(super) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

impl EditorState {
    /// Goes back to the selections before the last cursor move (or forward again).
    pub(super) fn restore_selections(&mut self, is_undo: bool) -> Outcome {
        let history = &mut self.cursor_history;
        let (from, to) =
            if is_undo { (&mut history.undo, &mut history.redo) } else { (&mut history.redo, &mut history.undo) };
        let Some(restored) = from.pop() else { return Outcome::default() };
        to.push(self.selections.clone());
        let restored = restored.clamped(&self.text);
        self.select_unrecorded(restored)
    }
}

#[cfg(test)]
mod tests {
    use crate::motion::Motion;
    use crate::state::{Command, EditorState};

    fn cursor_offset(state: &EditorState) -> usize {
        state.selections().primary().head
    }

    fn moved_right_twice() -> EditorState {
        let mut state = EditorState::new("abcdef");
        state.execute(Command::Move { motion: Motion::Right, extend: false });
        state.execute(Command::Move { motion: Motion::Right, extend: false });
        state
    }

    #[test]
    fn cursor_undo_goes_back_one_cursor_move() {
        let mut state = moved_right_twice();

        state.execute(Command::CursorUndo);

        assert_eq!(cursor_offset(&state), 1);
    }

    #[test]
    fn cursor_redo_goes_forward_again() {
        let mut state = moved_right_twice();
        state.execute(Command::CursorUndo);
        state.execute(Command::CursorUndo);

        state.execute(Command::CursorRedo);

        assert_eq!(cursor_offset(&state), 1);
    }

    #[test]
    fn an_edit_forgets_the_cursor_moves() {
        let mut state = moved_right_twice();
        state.execute(Command::Type("x".into()));

        state.execute(Command::CursorUndo);

        assert_eq!(cursor_offset(&state), 3);
    }

    #[test]
    fn a_move_after_a_cursor_undo_forgets_the_redo() {
        let mut state = moved_right_twice();
        state.execute(Command::CursorUndo);
        state.execute(Command::Move { motion: Motion::Left, extend: false });

        state.execute(Command::CursorRedo);

        assert_eq!(cursor_offset(&state), 0);
    }

    #[test]
    fn cursor_undo_works_in_a_read_only_editor() {
        let mut state = moved_right_twice();
        state.config.read_only = true;

        let outcome = state.execute(Command::CursorUndo);

        assert!(!outcome.rejected_read_only);
        assert_eq!(cursor_offset(&state), 1);
    }
}
