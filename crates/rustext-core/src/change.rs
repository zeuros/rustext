//! Edits as values: a `ChangeSet` is a list of replacements on one version of a text, which can
//! be applied, inverted, and used to map offsets from the old version to the new one (D7).

use crate::text::Text;

/// Which side an offset sticks to when text is inserted exactly at it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bias {
    /// Stays before the inserted text.
    Before,
    /// Moves after the inserted text.
    After,
}

/// Replaces the chars `start..end` of the old text with `insert`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Change {
    pub start: usize,
    pub end: usize,
    pub insert: String,
}

impl Change {
    pub fn new(start: usize, end: usize, insert: impl Into<String>) -> Self {
        Self { start: start.min(end), end: start.max(end), insert: insert.into() }
    }

    pub fn insert(offset: usize, insert: impl Into<String>) -> Self {
        Self::new(offset, offset, insert)
    }

    pub fn delete(start: usize, end: usize) -> Self {
        Self::new(start, end, "")
    }

    pub fn inserted_len(&self) -> usize {
        self.insert.chars().count()
    }

    pub fn deleted_len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_noop(&self) -> bool {
        self.start == self.end && self.insert.is_empty()
    }
}

/// Sorted, non-overlapping changes on one version of a text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeSet {
    changes: Vec<Change>,
}

impl ChangeSet {
    /// Sorts the changes and merges overlapping ones (their inserts are concatenated in order).
    /// Changes that only touch (one ends where the next starts) stay separate.
    pub fn new(changes: impl IntoIterator<Item = Change>) -> Self {
        let mut sorted: Vec<Change> = changes.into_iter().filter(|change| !change.is_noop()).collect();
        sorted.sort_by_key(|change| (change.start, change.end));

        let mut merged: Vec<Change> = Vec::with_capacity(sorted.len());
        for change in sorted {
            match merged.last_mut() {
                Some(previous) if change.start < previous.end => {
                    previous.end = previous.end.max(change.end);
                    previous.insert.push_str(&change.insert);
                }
                _ => merged.push(change),
            }
        }
        Self { changes: merged }
    }

    pub fn single(change: Change) -> Self {
        Self::new([change])
    }

    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// The text with every change applied.
    pub fn apply(&self, text: &Text) -> Text {
        text.replaced(self.changes.iter().rev().map(|change| (change.start, change.end, change.insert.clone())))
    }

    /// The change set that turns the new text back into `original` (the text this set applies to).
    pub fn invert(&self, original: &Text) -> ChangeSet {
        let mut length_delta: isize = 0;
        let inverted = self.changes.iter().map(|change| {
            let new_start = offset_plus(change.start, length_delta);
            length_delta += change.inserted_len() as isize - change.deleted_len() as isize;
            Change::new(new_start, new_start + change.inserted_len(), original.slice(change.start, change.end))
        });
        ChangeSet { changes: inverted.collect() }
    }

    /// Where `offset` of the old text is in the new text.
    ///
    /// An offset inside a replaced range goes to the range's start (`Bias::Before`) or after the
    /// inserted text (`Bias::After`); one at the very start of a replaced range stays before it,
    /// one at its very end goes after it. Only a pure insertion at the offset uses the bias.
    pub fn map_offset(&self, offset: usize, bias: Bias) -> usize {
        let mut length_delta: isize = 0;
        for change in &self.changes {
            if offset < change.start {
                break;
            }
            let new_start = offset_plus(change.start, length_delta);
            let is_insertion = change.start == change.end;
            if offset == change.start && !is_insertion {
                return new_start;
            }
            if offset < change.end || (is_insertion && offset == change.start) {
                return match bias {
                    Bias::Before => new_start,
                    Bias::After => new_start + change.inserted_len(),
                };
            }
            length_delta += change.inserted_len() as isize - change.deleted_len() as isize;
        }
        offset_plus(offset, length_delta)
    }

    /// Lines of the old text touched by the changes, as `(first_line, last_line)` per change.
    pub fn touched_lines<'a>(&'a self, text: &'a Text) -> impl Iterator<Item = (usize, usize)> + 'a {
        self.changes.iter().map(|change| (text.line_of_offset(change.start), text.line_of_offset(change.end)))
    }

    /// The first line of the old text a change touches.
    pub fn first_touched_line(&self, text: &Text) -> Option<usize> {
        self.changes.first().map(|change| text.line_of_offset(change.start))
    }
}

fn offset_plus(offset: usize, delta: isize) -> usize {
    offset.saturating_add_signed(delta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_several_changes_against_the_old_offsets() {
        let text = Text::new("hello world");
        let changes = ChangeSet::new([Change::new(6, 11, "rust"), Change::insert(0, ">> ")]);

        assert_eq!(changes.apply(&text).to_string(), ">> hello rust");
    }

    #[test]
    fn invert_restores_the_original() {
        let text = Text::new("one two three");
        let changes = ChangeSet::new([Change::delete(0, 4), Change::new(8, 13, "3"), Change::insert(4, "2:")]);
        let edited = changes.apply(&text);

        assert_eq!(edited.to_string(), "2:two 3");
        assert_eq!(changes.invert(&text).apply(&edited), text);
    }

    #[test]
    fn maps_offsets_around_changes() {
        let changes = ChangeSet::new([Change::new(2, 4, "xyz")]);

        assert_eq!(changes.map_offset(1, Bias::Before), 1);
        assert_eq!(changes.map_offset(2, Bias::After), 2);
        assert_eq!(changes.map_offset(3, Bias::Before), 2);
        assert_eq!(changes.map_offset(3, Bias::After), 5);
        assert_eq!(changes.map_offset(4, Bias::Before), 5);
        assert_eq!(changes.map_offset(6, Bias::Before), 7);
    }

    #[test]
    fn insertion_at_an_offset_follows_the_bias() {
        let changes = ChangeSet::single(Change::insert(3, "ab"));

        assert_eq!(changes.map_offset(3, Bias::Before), 3);
        assert_eq!(changes.map_offset(3, Bias::After), 5);
    }

    #[test]
    fn overlapping_changes_merge() {
        let changes = ChangeSet::new([Change::delete(0, 5), Change::delete(3, 6)]);

        assert_eq!(changes.changes(), &[Change::delete(0, 6)]);
    }

    #[test]
    fn noop_changes_are_dropped() {
        assert!(ChangeSet::new([Change::insert(2, "")]).is_empty());
    }
}
