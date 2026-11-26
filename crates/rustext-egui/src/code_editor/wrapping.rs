//! Word wrap state: the wrap points of every line, kept across frames (DECISIONS D20).

use rustext_core::wrap::wrap_points;
use rustext_core::{EditedLines, Text};

/// Where lines wrap. `width_columns == 0` means no wrapping.
#[derive(Clone, Debug, Default)]
pub struct WrapState {
    width_columns: usize,
    tab_size: usize,
    /// `points[line]`: the start columns of the line's continuation rows.
    points: Vec<Vec<usize>>,
    /// Changes whenever any line's points change, to key caches built from them.
    generation: u64,
}

impl WrapState {
    pub fn is_wrapping(&self) -> bool {
        self.width_columns > 0
    }

    pub fn width_columns(&self) -> usize {
        self.width_columns
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn points(&self, line: usize) -> &[usize] {
        self.points.get(line).map_or(&[], Vec::as_slice)
    }

    /// Wraps every line again when the width, the tab size or the line count changed (a resize,
    /// a new text); otherwise keeps what edits maintained.
    pub fn ensure(&mut self, text: &Text, width_columns: usize, tab_size: usize) {
        if width_columns == 0 {
            if self.is_wrapping() {
                *self = WrapState { generation: self.generation + 1, ..WrapState::default() };
            }
            return;
        }
        let is_current =
            self.width_columns == width_columns && self.tab_size == tab_size && self.points.len() == text.line_count();
        if is_current {
            return;
        }
        self.width_columns = width_columns;
        self.tab_size = tab_size;
        self.points = text
            .raw_lines()
            .map(|raw| wrap_points(raw.trim_end_matches(['\n', '\r']), width_columns, tab_size))
            .collect();
        self.points.resize(text.line_count(), Vec::new());
        self.generation += 1;
    }

    /// Wraps the lines an edit touched again (`text` is the text after the edit).
    pub fn follow_edit(&mut self, edited: &EditedLines, text: &Text) {
        if !self.is_wrapping() || edited.old_last_line >= self.points.len() {
            self.points.clear();
            return;
        }
        let new_points = (edited.first_line..=edited.new_last_line.min(text.last_line()))
            .map(|line| wrap_points(&text.line_content(line), self.width_columns, self.tab_size));
        self.points.splice(edited.first_line..=edited.old_last_line, new_points);
        self.generation += 1;
    }
}
