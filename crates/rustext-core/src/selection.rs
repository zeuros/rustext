//! Selections in char offsets: `anchor` is where the selection started, `head` where the cursor is.
//! `Selections` holds one or more of them, always sorted and without overlaps, one of them primary.

use crate::change::{Bias, ChangeSet};
use crate::position::{Position, Range};
use crate::text::Text;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
    /// Visual column the cursor tries to keep on vertical moves (tabs expanded).
    pub preferred_visual_column: Option<usize>,
}

impl Selection {
    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head, preferred_visual_column: None }
    }

    pub fn cursor(offset: usize) -> Self {
        Self::new(offset, offset)
    }

    pub fn start(&self) -> usize {
        self.anchor.min(self.head)
    }

    pub fn end(&self) -> usize {
        self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    pub fn is_reversed(&self) -> bool {
        self.head < self.anchor
    }

    pub fn range(&self, text: &Text) -> Range {
        Range::new(text.position(self.start()), text.position(self.end()))
    }

    pub fn head_position(&self, text: &Text) -> Position {
        text.position(self.head)
    }

    /// The selection with its head moved: extending keeps the anchor, otherwise it collapses.
    pub fn moved_to(&self, head: usize, extend: bool) -> Self {
        Self::new(if extend { self.anchor } else { head }, head)
    }

    pub fn with_preferred_visual_column(self, preferred_visual_column: Option<usize>) -> Self {
        Self { preferred_visual_column, ..self }
    }

    /// Lines the selection covers. A selection ending at column 0 of a line doesn't cover that
    /// line (selecting whole lines with Shift+Down ends there).
    pub fn covered_lines(&self, text: &Text) -> (usize, usize) {
        let first_line = text.line_of_offset(self.start());
        let end_position = text.position(self.end());
        let last_line = if end_position.column == 0 && end_position.line > first_line {
            end_position.line - 1
        } else {
            end_position.line
        };
        (first_line, last_line)
    }

    /// The selection in the text after `changes`. A cursor sticks after text inserted at it.
    pub fn map(&self, changes: &ChangeSet) -> Self {
        if self.is_empty() {
            let offset = changes.map_offset(self.head, Bias::After);
            return Self::cursor(offset);
        }
        let (start_bias, end_bias) = (Bias::After, Bias::Before);
        let (anchor_bias, head_bias) = if self.is_reversed() { (end_bias, start_bias) } else { (start_bias, end_bias) };
        Self::new(changes.map_offset(self.anchor, anchor_bias), changes.map_offset(self.head, head_bias))
    }
}

/// One or more selections, sorted by start, never overlapping. `primary` is the one that
/// keyboard-only commands (find seeds, reveal) follow: the most recently added cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selections {
    ranges: Vec<Selection>,
    primary_index: usize,
}

impl Selections {
    pub fn single(selection: Selection) -> Self {
        Self { ranges: vec![selection], primary_index: 0 }
    }

    pub fn cursor(offset: usize) -> Self {
        Self::single(Selection::cursor(offset))
    }

    /// Builds normalized selections. `primary_index` points into `ranges` before normalizing.
    pub fn new(ranges: Vec<Selection>, primary_index: usize) -> Self {
        assert!(!ranges.is_empty(), "Selections needs at least one selection");
        let primary = ranges[primary_index.min(ranges.len() - 1)];
        normalized(ranges, primary)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Selection> {
        self.ranges.iter()
    }

    pub fn as_slice(&self) -> &[Selection] {
        &self.ranges
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn primary(&self) -> Selection {
        self.ranges[self.primary_index]
    }

    pub fn primary_index(&self) -> usize {
        self.primary_index
    }

    pub fn is_multiple(&self) -> bool {
        self.ranges.len() > 1
    }

    /// Applies `transform` to each selection, keeping the primary one primary.
    pub fn map_each(&self, transform: impl FnMut(&Selection) -> Selection) -> Self {
        Self::new(self.ranges.iter().map(transform).collect(), self.primary_index)
    }

    /// The selections in the text after `changes`.
    pub fn map(&self, changes: &ChangeSet) -> Self {
        self.map_each(|selection| selection.map(changes))
    }

    /// Adds a selection and makes it primary.
    pub fn added(&self, selection: Selection) -> Self {
        let mut ranges = self.ranges.clone();
        ranges.push(selection);
        let primary_index = ranges.len() - 1;
        Self::new(ranges, primary_index)
    }

    pub fn only_primary(&self) -> Self {
        Self::single(self.primary())
    }

    /// Selections clamped into the text, never inside a line ending.
    pub fn clamped(&self, text: &Text) -> Self {
        self.map_each(|selection| Selection {
            anchor: text.clamp_offset(selection.anchor),
            head: text.clamp_offset(selection.head),
            ..*selection
        })
    }
}

impl Default for Selections {
    fn default() -> Self {
        Self::cursor(0)
    }
}

/// Sorts and merges overlapping selections. Two cursors at the same offset merge; two non-empty
/// selections that only touch stay apart. A merge keeps the direction of the primary selection
/// when it is part of it.
fn normalized(mut ranges: Vec<Selection>, primary: Selection) -> Selections {
    ranges.sort_by_key(|selection| (selection.start(), selection.end()));

    let mut merged: Vec<Selection> = Vec::with_capacity(ranges.len());
    let mut primary_index = 0;
    for selection in ranges {
        let is_primary = selection == primary;
        match merged.last_mut() {
            Some(previous) if overlaps(previous, &selection) => {
                let start = previous.start().min(selection.start());
                let end = previous.end().max(selection.end());
                let reversed = if is_primary { selection.is_reversed() } else { previous.is_reversed() };
                *previous = if reversed { Selection::new(end, start) } else { Selection::new(start, end) };
                if is_primary {
                    primary_index = merged.len() - 1;
                }
            }
            _ => {
                if is_primary {
                    primary_index = merged.len();
                }
                merged.push(selection);
            }
        }
    }
    Selections { ranges: merged, primary_index }
}

fn overlaps(first: &Selection, second: &Selection) -> bool {
    let both_touch_as_cursors = first.is_empty() || second.is_empty();
    second.start() < first.end() || (second.start() == first.end() && both_touch_as_cursors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change::Change;

    #[test]
    fn sorts_and_merges_overlapping() {
        let selections = Selections::new(
            vec![Selection::new(10, 12), Selection::new(1, 4), Selection::new(3, 6), Selection::cursor(12)],
            0,
        );

        assert_eq!(selections.as_slice(), &[Selection::new(1, 6), Selection::new(10, 12)]);
        assert_eq!(selections.primary(), Selection::new(10, 12));
    }

    #[test]
    fn touching_selections_stay_apart() {
        let selections = Selections::new(vec![Selection::new(0, 2), Selection::new(2, 4)], 0);

        assert_eq!(selections.len(), 2);
    }

    #[test]
    fn cursor_sticks_after_text_inserted_at_it() {
        let changes = ChangeSet::single(Change::insert(2, "abc"));

        assert_eq!(Selection::cursor(2).map(&changes), Selection::cursor(5));
    }

    #[test]
    fn selection_does_not_grow_on_insertions_at_its_edges() {
        let changes = ChangeSet::new([Change::insert(2, "x"), Change::insert(4, "y")]);

        assert_eq!(Selection::new(2, 4).map(&changes), Selection::new(3, 5));
        assert_eq!(Selection::new(4, 2).map(&changes), Selection::new(5, 3));
    }

    #[test]
    fn covered_lines_excludes_a_line_reached_at_column_zero() {
        let text = Text::new("a\nb\nc");

        assert_eq!(Selection::new(0, 4).covered_lines(&text), (0, 1));
        assert_eq!(Selection::new(0, 5).covered_lines(&text), (0, 2));
        assert_eq!(Selection::cursor(2).covered_lines(&text), (1, 1));
    }
}
