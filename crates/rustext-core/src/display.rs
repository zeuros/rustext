//! Display rows: what an editor paints, row by row, when it is not one row per line
//! (DECISIONS D17). Padding rows align the two sides of a diff, collapsed rows stand for hidden
//! lines, foreign rows show lines of another text (an inline diff's removed lines).

use crate::style::Rgba;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RowContent {
    /// A line of the editor's own text (its first row when it wraps).
    Line(usize),
    /// A continuation row of a wrapped line, from `start_column` to the next row's start.
    Wrapped { line: usize, start_column: usize },
    /// An empty row.
    Padding,
    /// One row standing for the hidden lines `first_line..=last_line` of the editor's text.
    Collapsed { first_line: usize, last_line: usize },
    /// A line of the foreign text the editor is given (read-only).
    Foreign(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DisplayRow {
    pub content: RowContent,
    /// Background of the whole row, gutter included.
    pub background: Option<Rgba>,
    /// A char shown in the gutter before the text (`+`, `-`).
    pub marker: Option<char>,
}

impl DisplayRow {
    pub const fn new(content: RowContent) -> Self {
        Self { content, background: None, marker: None }
    }

    pub const fn with_background(self, background: Rgba) -> Self {
        Self { background: Some(background), ..self }
    }

    pub const fn with_marker(self, marker: char) -> Self {
        Self { marker: Some(marker), ..self }
    }
}

/// The rows of an editor. Lines of its text absent from the rows are hidden.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DisplayLayout {
    rows: Vec<DisplayRow>,
    /// `row_of_line[line]`: the row showing `line`, or the row of the collapsed block hiding it.
    row_of_line: Vec<usize>,
}

impl DisplayLayout {
    /// Rows for a text of `line_count` lines. Every line must appear in order, as a `Line` row
    /// or inside a `Collapsed` row; a line appearing nowhere maps to the row after it.
    pub fn new(rows: Vec<DisplayRow>, line_count: usize) -> Self {
        let mut row_of_line = vec![usize::MAX; line_count];
        for (row, display_row) in rows.iter().enumerate() {
            match display_row.content {
                RowContent::Line(line) if line < line_count => row_of_line[line] = row,
                RowContent::Collapsed { first_line, last_line } => {
                    let last_line = last_line.min(line_count.saturating_sub(1));
                    row_of_line.iter_mut().take(last_line + 1).skip(first_line).for_each(|line_row| *line_row = row);
                }
                _ => {}
            }
        }
        let mut next_row = rows.len().saturating_sub(1);
        for row in row_of_line.iter_mut().rev() {
            if *row == usize::MAX {
                *row = next_row;
            } else {
                next_row = *row;
            }
        }
        Self { rows, row_of_line }
    }

    /// One row per line, nothing else.
    pub fn identity(line_count: usize) -> Self {
        Self::new((0..line_count).map(|line| DisplayRow::new(RowContent::Line(line))).collect(), line_count)
    }

    pub fn rows(&self) -> &[DisplayRow] {
        &self.rows
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn row(&self, row: usize) -> Option<&DisplayRow> {
        self.rows.get(row)
    }

    pub fn row_of_line(&self, line: usize) -> usize {
        self.row_of_line.get(line).copied().unwrap_or(self.rows.len().saturating_sub(1))
    }

    /// Whether `line` has a row of its own (not padding, not collapsed away).
    pub fn is_line_shown(&self, line: usize) -> bool {
        self.rows.get(self.row_of_line(line)).is_some_and(|row| row.content == RowContent::Line(line))
    }

    /// The text line nearest to `row`: its own line, the first hidden one, or the next line
    /// below a padding or foreign row (the previous one at the end).
    pub fn nearest_line(&self, row: usize) -> usize {
        let line_at = |row: &DisplayRow| match row.content {
            RowContent::Line(line) | RowContent::Wrapped { line, .. } => Some(line),
            RowContent::Collapsed { first_line, .. } => Some(first_line),
            RowContent::Padding | RowContent::Foreign(_) => None,
        };
        let row = row.min(self.rows.len().saturating_sub(1));
        self.rows[row..]
            .iter()
            .find_map(line_at)
            .or_else(|| self.rows[..row].iter().rev().find_map(line_at))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> DisplayLayout {
        DisplayLayout::new(
            vec![
                DisplayRow::new(RowContent::Line(0)),
                DisplayRow::new(RowContent::Padding),
                DisplayRow::new(RowContent::Collapsed { first_line: 1, last_line: 3 }),
                DisplayRow::new(RowContent::Foreign(7)),
                DisplayRow::new(RowContent::Line(4)),
            ],
            5,
        )
    }

    #[test]
    fn maps_lines_to_rows() {
        let layout = layout();

        assert_eq!(layout.row_of_line(0), 0);
        assert_eq!(layout.row_of_line(2), 2);
        assert_eq!(layout.row_of_line(4), 4);
        assert!(layout.is_line_shown(4));
        assert!(!layout.is_line_shown(2));
    }

    #[test]
    fn nearest_line_looks_below_then_above() {
        let layout = layout();

        assert_eq!(layout.nearest_line(1), 1);
        assert_eq!(layout.nearest_line(3), 4);
        assert_eq!(layout.nearest_line(99), 4);
    }

    #[test]
    fn identity_has_one_row_per_line() {
        let layout = DisplayLayout::identity(3);

        assert_eq!(layout.row_count(), 3);
        assert_eq!(layout.nearest_line(2), 2);
    }
}
