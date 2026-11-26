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

use rustext_core::{Selection, Selections};

use super::CodeEditor;

impl CodeEditor<'_> {
    /// Visual column of char `column` within the row segment starting at `start_column`
    /// (continuation rows start after their indent).
    fn visual_column_in_row(&self, line: usize, start_column: usize, column: usize) -> usize {
        let content = self.state.text().line_content(line);
        let indentation = self.state.config.indentation;
        let indent = if start_column > 0 {
            rustext_core::wrap::continuation_indent(&content, self.view.wraps.width_columns(), indentation.tab_size)
        } else {
            0
        };
        let segment: String = content.chars().skip(start_column).collect();
        indent + indentation.visual_column(&segment, column.saturating_sub(start_column))
    }

    /// Up / Down when wrapping: to the row above or below, at the same visual column.
    pub(super) fn selections_moved_by_row(&self, down: bool, extend: bool) -> Selections {
        let text = self.state.text();
        let indentation = self.state.config.indentation;
        self.state.selections().map_each(|selection| {
            let head = text.position(selection.head);
            let row = self.row_of_position(head.line, head.column);
            let Some((line, start_column, _)) = self.row_segment(row) else { return *selection };
            let visual_column = selection
                .preferred_visual_column
                .unwrap_or_else(|| self.visual_column_in_row(line, start_column, head.column));
            let target_row = if down { (row + 1 < self.row_count()).then_some(row + 1) } else { row.checked_sub(1) };
            let target = target_row.and_then(|target_row| self.row_segment(target_row));
            let offset = match target {
                Some((target_line, target_start, target_end)) => {
                    let content = text.line_content(target_line);
                    let line_len = content.chars().count();
                    let segment_end = target_end.min(line_len);
                    let is_last_segment = segment_end == line_len;
                    let segment: String = content.chars().skip(target_start).take(segment_end - target_start).collect();
                    let indent = self.visual_column_in_row(target_line, target_start, target_start);
                    let column = indentation.column_at_visual(&segment, visual_column.saturating_sub(indent));
                    let last_column = if is_last_segment {
                        segment_end - target_start
                    } else {
                        (segment_end - target_start).saturating_sub(1)
                    };
                    text.offset(rustext_core::Position::new(target_line, target_start + column.min(last_column)))
                }
                None if down => text.len_chars(),
                None => 0,
            };
            Selection { preferred_visual_column: Some(visual_column), ..selection.moved_to(offset, extend) }
        })
    }
}
