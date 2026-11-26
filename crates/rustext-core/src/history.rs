//! Undo / redo. Each entry keeps the change sets it is made of with their inverses, and the
//! selections before and after, so undoing restores both the text and the cursors.

use crate::change::ChangeSet;
use crate::selection::Selections;
use crate::text::Text;

/// What kind of edit a transaction is, to group consecutive typing into one undo step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EditKind {
    Typing,
    Deleting,
    /// Everything else (paste, line moves, replace all…): always its own undo step.
    Other,
}

#[derive(Clone, Debug)]
struct Step {
    changes: ChangeSet,
    inverse: ChangeSet,
}

#[derive(Clone, Debug)]
struct Entry {
    steps: Vec<Step>,
    selections_before: Selections,
    selections_after: Selections,
    kind: EditKind,
    /// Identifies the text this entry leaves.
    version_after: u64,
}

/// The result of an undo or redo: the change sets to apply in order, and the selections after.
#[derive(Clone, Debug)]
pub struct Replay {
    pub changes: Vec<ChangeSet>,
    pub selections: Selections,
}

#[derive(Clone, Debug, Default)]
pub struct History {
    undo_entries: Vec<Entry>,
    redo_entries: Vec<Entry>,
    /// When false, the next edit starts a new entry even if it is the same kind as the last.
    is_last_entry_open: bool,
    last_version: u64,
}

impl History {
    /// Records an edit. Typing after typing (and deleting after deleting) joins the last entry,
    /// unless an undo stop was pushed in between.
    pub fn record(
        &mut self,
        original: &Text,
        changes: ChangeSet,
        selections_before: Selections,
        selections_after: Selections,
        kind: EditKind,
    ) {
        let step = Step { inverse: changes.invert(original), changes };
        self.redo_entries.clear();
        self.last_version += 1;
        let version_after = self.last_version;

        let joins_last_entry = self.is_last_entry_open && kind != EditKind::Other;
        match self.undo_entries.last_mut() {
            Some(last) if joins_last_entry && last.kind == kind => {
                last.steps.push(step);
                last.selections_after = selections_after;
                last.version_after = version_after;
            }
            _ => self.undo_entries.push(Entry {
                steps: vec![step],
                selections_before,
                selections_after,
                kind,
                version_after,
            }),
        }
        self.is_last_entry_open = kind != EditKind::Other;
    }

    /// The next edit starts a new undo entry (cursor moved, focus lost, explicit stop).
    pub fn push_undo_stop(&mut self) {
        self.is_last_entry_open = false;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_entries.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_entries.is_empty()
    }

    pub fn undo(&mut self) -> Option<Replay> {
        let entry = self.undo_entries.pop()?;
        let replay = Replay {
            changes: entry.steps.iter().rev().map(|step| step.inverse.clone()).collect(),
            selections: entry.selections_before.clone(),
        };
        self.redo_entries.push(entry);
        self.is_last_entry_open = false;
        Some(replay)
    }

    pub fn redo(&mut self) -> Option<Replay> {
        let entry = self.redo_entries.pop()?;
        let replay = Replay {
            changes: entry.steps.iter().map(|step| step.changes.clone()).collect(),
            selections: entry.selections_after.clone(),
        };
        self.undo_entries.push(entry);
        self.is_last_entry_open = false;
        Some(replay)
    }

    /// Identifies the text's content in history: back to the same value after undoing to it,
    /// unlike a counter of edits (Monaco's `getAlternativeVersionId`). Keep the value at save
    /// time and compare to know whether the document is dirty.
    pub fn version(&self) -> u64 {
        self.undo_entries.last().map_or(0, |entry| entry.version_after)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change::Change;

    fn record(history: &mut History, text: &Text, change: Change, kind: EditKind) -> Text {
        let changes = ChangeSet::single(change);
        let edited = changes.apply(text);
        history.record(text, changes, Selections::default(), Selections::default(), kind);
        edited
    }

    fn replay(text: &Text, replay: Replay) -> Text {
        replay.changes.iter().fold(text.clone(), |text, changes| changes.apply(&text))
    }

    #[test]
    fn typing_joins_one_entry_until_a_stop() {
        let mut history = History::default();
        let text = Text::new("");
        let text = record(&mut history, &text, Change::insert(0, "a"), EditKind::Typing);
        let text = record(&mut history, &text, Change::insert(1, "b"), EditKind::Typing);
        history.push_undo_stop();
        let text = record(&mut history, &text, Change::insert(2, "c"), EditKind::Typing);

        let text = replay(&text, history.undo().unwrap());
        assert_eq!(text.to_string(), "ab");
        let text = replay(&text, history.undo().unwrap());
        assert_eq!(text.to_string(), "");
        assert!(!history.can_undo());

        let text = replay(&text, history.redo().unwrap());
        assert_eq!(text.to_string(), "ab");
    }

    #[test]
    fn version_comes_back_after_undo() {
        let mut history = History::default();
        let text = Text::new("");
        let text = record(&mut history, &text, Change::insert(0, "a"), EditKind::Other);
        let saved_version = history.version();
        let text = record(&mut history, &text, Change::insert(1, "b"), EditKind::Other);

        assert_ne!(history.version(), saved_version);
        replay(&text, history.undo().unwrap());
        assert_eq!(history.version(), saved_version);
        history.undo();
        assert_eq!(history.version(), 0);
    }

    #[test]
    fn a_new_edit_clears_redo() {
        let mut history = History::default();
        let text = Text::new("");
        let text = record(&mut history, &text, Change::insert(0, "a"), EditKind::Other);
        let text = replay(&text, history.undo().unwrap());
        record(&mut history, &text, Change::insert(0, "b"), EditKind::Other);

        assert!(!history.can_redo());
    }
}
