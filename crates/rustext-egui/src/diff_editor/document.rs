//! The diff document's own behaviour: its texts, the diff, where the cursors are in it.

use super::*;

impl DiffDocument {
    pub fn new(old: &str, new: &str) -> Self {
        let read_only = EditingConfig { read_only: true, ..EditingConfig::default() };
        let mut document = Self {
            old: EditorState::new(old).with_config(read_only),
            new: EditorState::new(new),
            old_view: EditorView::default(),
            new_view: EditorView::default(),
            diff: LineDiff::default(),
            diff_options: DiffOptions::default(),
            is_diff_stale: true,
            expanded_old_lines: Vec::new(),
            display: None,
            focused_side: DiffSide::New,
            selected_removed_lines: BTreeSet::new(),
            last_clicked_removed_line: None,
            dragged_split_ratio: None,
            shows_accessible_viewer: false,
        };
        document.refresh_diff(&DiffOptions::default());
        document
    }

    /// Replaces both texts (a new file to show, or the files changed on disk).
    pub fn set_texts(&mut self, old: Text, new: Text) {
        self.old.set_text(old);
        self.new.set_text(new);
        self.expanded_old_lines.clear();
        self.selected_removed_lines.clear();
        self.is_diff_stale = true;
    }

    pub fn diff(&self) -> &LineDiff {
        &self.diff
    }

    pub fn focused_side(&self) -> DiffSide {
        self.focused_side
    }

    pub(super) fn refresh_diff(&mut self, options: &DiffOptions) {
        if !self.is_diff_stale && self.diff_options == *options {
            return;
        }
        self.diff = diff_lines(self.old.text(), self.new.text(), options);
        self.diff_options = *options;
        self.is_diff_stale = false;
        self.display = None;
    }

    /// The changed lines the user picked, for staging, unstaging or discarding: the lines the
    /// selections of the last focused side cover (a cursor alone picks its line), plus removed
    /// lines picked in an inline view.
    pub fn selected_lines(&self) -> LineSelection {
        let covered = |state: &EditorState| -> BTreeSet<usize> {
            let text = state.text();
            state
                .selections()
                .iter()
                .flat_map(|selection| {
                    let (first_line, last_line) = selection.covered_lines(text);
                    first_line..=last_line
                })
                .collect()
        };
        let mut selection = match self.focused_side {
            DiffSide::Old => LineSelection { old_lines: covered(&self.old), new_lines: BTreeSet::new() },
            DiffSide::New => LineSelection { old_lines: BTreeSet::new(), new_lines: covered(&self.new) },
        };
        selection.old_lines.extend(self.selected_removed_lines.iter().copied());
        selection
    }

    /// Index of the hunk holding the primary cursor of the focused side, if any.
    pub fn hunk_at_cursor(&self) -> Option<usize> {
        let (state, is_old) = match self.focused_side {
            DiffSide::Old => (&self.old, true),
            DiffSide::New => (&self.new, false),
        };
        let line = state.text().line_of_offset(state.selections().primary().head);
        self.diff.hunks.iter().position(|hunk| {
            let range = if is_old { hunk.old } else { hunk.new };
            range.contains(line) || (range.is_empty() && range.start == line)
        })
    }

    /// Moves the new side's cursor to the next (or previous) change, wrapping around (F7).
    pub fn go_to_change(&mut self, forward: bool) {
        if self.diff.hunks.is_empty() {
            return;
        }
        let text = self.new.text().clone();
        let cursor_line = text.line_of_offset(self.new.selections().primary().head);
        let hunk_starts: Vec<usize> = self.diff.hunks.iter().map(|hunk| hunk.new.start).collect();
        let target_line = if forward {
            hunk_starts.iter().find(|&&start| start > cursor_line).or(hunk_starts.first())
        } else {
            hunk_starts.iter().rev().find(|&&start| start < cursor_line).or(hunk_starts.last())
        };
        let Some(&target_line) = target_line else { return };
        let offset = text.line_start(target_line.min(text.last_line()));
        self.new.execute(Command::SetSelections(Selections::cursor(offset)));
        self.focused_side = DiffSide::New;
        self.new_view.reveal_cursor();
    }

    /// Puts the cursor of `side` at the start of the `index`th moved block there, the other end
    /// of the move whose lens was clicked (D30).
    pub(super) fn go_to_move(&mut self, index: usize, side: DiffSide) {
        let Some(moved) = self.diff.moves.get(index).copied() else { return };
        let (state, view, line) = match side {
            DiffSide::Old => (&mut self.old, &mut self.old_view, moved.old.start),
            DiffSide::New => (&mut self.new, &mut self.new_view, moved.new.start),
        };
        let offset = state.text().line_start(line.min(state.text().last_line()));
        state.execute(Command::SetSelections(Selections::cursor(offset)));
        view.reveal_cursor();
        self.focused_side = side;
    }

    /// How the rows are collapsed and folded now.
    pub(super) fn collapsing(&self, options: Option<CollapseOptions>) -> Collapsing {
        Collapsing {
            options,
            expanded_old_lines: self.expanded_old_lines.clone(),
            old_folds: self.old_view.folds.folded_ranges(),
            new_folds: self.new_view.folds.folded_ranges(),
        }
    }

    pub(super) fn expand_collapsed(&mut self, side: DiffSide, first_line: usize) {
        let old_line = match side {
            DiffSide::Old => Some(first_line),
            DiffSide::New => side_by_side_rows(&self.diff)
                .iter()
                .find(|row| row.kind == RowKind::Unchanged && row.new_line == Some(first_line))
                .and_then(|row| row.old_line),
        };
        if let Some(old_line) = old_line {
            self.expanded_old_lines.push(old_line);
            self.display = None;
        }
    }

    pub(super) fn click_removed_line(&mut self, line: usize, extends: bool) {
        match (extends, self.last_clicked_removed_line) {
            (true, Some(anchor)) => self.selected_removed_lines.extend(anchor.min(line)..=anchor.max(line)),
            _ => {
                self.selected_removed_lines = BTreeSet::from([line]);
                self.last_clicked_removed_line = Some(line);
            }
        }
        self.focused_side = DiffSide::New;
    }
}
