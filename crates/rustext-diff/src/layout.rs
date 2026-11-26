//! How a diff is laid out in rows: side by side (with padding where one side has no line),
//! inline (removed lines above added ones), and with unchanged regions collapsed. Pure
//! functions of a `LineDiff`; the widget only paints rows.

use crate::line_diff::LineDiff;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RowKind {
    Unchanged,
    /// Only the old side has a line here.
    Removed,
    /// Only the new side has a line here.
    Added,
    /// Both sides have a line, part of the same hunk.
    Modified,
}

/// One row of a side-by-side view. A missing line is padding on that side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SideBySideRow {
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub kind: RowKind,
    /// Index of the hunk the row belongs to.
    pub hunk_index: Option<usize>,
}

/// One row of an inline (unified) view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InlineRow {
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub kind: RowKind,
    pub hunk_index: Option<usize>,
}

/// Rows of a side-by-side view: a hunk's old and new lines face each other, the shorter side
/// padded after its lines.
pub fn side_by_side_rows(diff: &LineDiff) -> Vec<SideBySideRow> {
    let mut rows = Vec::with_capacity(diff.old_line_count.max(diff.new_line_count));
    let (mut old_line, mut new_line) = (0, 0);
    for (hunk_index, hunk) in diff.hunks.iter().enumerate() {
        while old_line < hunk.old.start {
            rows.push(SideBySideRow {
                old_line: Some(old_line),
                new_line: Some(new_line),
                kind: RowKind::Unchanged,
                hunk_index: None,
            });
            old_line += 1;
            new_line += 1;
        }
        let height = hunk.old.len().max(hunk.new.len());
        for offset in 0..height {
            let old = (offset < hunk.old.len()).then_some(hunk.old.start + offset);
            let new = (offset < hunk.new.len()).then_some(hunk.new.start + offset);
            let kind = match (old, new) {
                (Some(_), Some(_)) => RowKind::Modified,
                (Some(_), None) => RowKind::Removed,
                _ => RowKind::Added,
            };
            rows.push(SideBySideRow { old_line: old, new_line: new, kind, hunk_index: Some(hunk_index) });
        }
        old_line = hunk.old.end;
        new_line = hunk.new.end;
    }
    while old_line < diff.old_line_count {
        rows.push(SideBySideRow {
            old_line: Some(old_line),
            new_line: Some(new_line),
            kind: RowKind::Unchanged,
            hunk_index: None,
        });
        old_line += 1;
        new_line += 1;
    }
    rows
}

/// Rows of an inline view: unchanged lines once, a hunk's removed lines then its added lines.
pub fn inline_rows(diff: &LineDiff) -> Vec<InlineRow> {
    let mut rows = Vec::with_capacity(diff.old_line_count + diff.added_line_count());
    let (mut old_line, mut new_line) = (0, 0);
    let unchanged = |old_line, new_line| InlineRow {
        old_line: Some(old_line),
        new_line: Some(new_line),
        kind: RowKind::Unchanged,
        hunk_index: None,
    };
    for (hunk_index, hunk) in diff.hunks.iter().enumerate() {
        while old_line < hunk.old.start {
            rows.push(unchanged(old_line, new_line));
            old_line += 1;
            new_line += 1;
        }
        let hunk_index = Some(hunk_index);
        rows.extend(hunk.old.iter().map(|line| InlineRow {
            old_line: Some(line),
            new_line: None,
            kind: RowKind::Removed,
            hunk_index,
        }));
        rows.extend(hunk.new.iter().map(|line| InlineRow {
            old_line: None,
            new_line: Some(line),
            kind: RowKind::Added,
            hunk_index,
        }));
        old_line = hunk.old.end;
        new_line = hunk.new.end;
    }
    while old_line < diff.old_line_count {
        rows.push(unchanged(old_line, new_line));
        old_line += 1;
        new_line += 1;
    }
    rows
}

/// A run of rows: shown, or collapsed into one "N hidden lines" row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Segment {
    Shown { start_row: usize, end_row: usize },
    Collapsed { start_row: usize, end_row: usize },
}

impl Segment {
    pub fn rows(&self) -> std::ops::Range<usize> {
        match *self {
            Segment::Shown { start_row, end_row } | Segment::Collapsed { start_row, end_row } => start_row..end_row,
        }
    }
}

/// Monaco's `hideUnchangedRegions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CollapseOptions {
    /// Unchanged rows kept around each change.
    pub context_rows: usize,
    /// Runs shorter than this stay shown (collapsing two lines saves nothing).
    pub minimum_collapsed_rows: usize,
}

impl Default for CollapseOptions {
    fn default() -> Self {
        Self { context_rows: 3, minimum_collapsed_rows: 3 }
    }
}

/// Splits rows into shown and collapsed segments: unchanged rows further than `context_rows`
/// from any change are collapsed, unless the run is in `expanded_start_rows` (the user
/// clicked it open; identified by the row it starts at).
pub fn collapse_unchanged(
    row_kinds: &[RowKind],
    options: &CollapseOptions,
    expanded_start_rows: &[usize],
) -> Vec<Segment> {
    let row_count = row_kinds.len();
    let mut is_shown = vec![false; row_count];
    for (row, kind) in row_kinds.iter().enumerate() {
        if *kind != RowKind::Unchanged {
            let first = row.saturating_sub(options.context_rows);
            let last = (row + options.context_rows).min(row_count.saturating_sub(1));
            is_shown[first..=last].iter_mut().for_each(|shown| *shown = true);
        }
    }

    let mut segments: Vec<Segment> = Vec::new();
    let mut row = 0;
    while row < row_count {
        let run_is_shown = is_shown[row];
        let start_row = row;
        while row < row_count && is_shown[row] == run_is_shown {
            row += 1;
        }
        let keeps_shown = run_is_shown
            || row - start_row < options.minimum_collapsed_rows
            || expanded_start_rows.contains(&start_row);
        let segment = if keeps_shown {
            Segment::Shown { start_row, end_row: row }
        } else {
            Segment::Collapsed { start_row, end_row: row }
        };
        match (segments.last_mut(), segment) {
            (Some(Segment::Shown { end_row, .. }), Segment::Shown { end_row: new_end_row, .. }) => {
                *end_row = new_end_row
            }
            _ => segments.push(segment),
        }
    }
    segments
}

/// The first row of the next change after `row` (wrapping around), for F7.
pub fn next_change_row(row_kinds: &[RowKind], row: usize) -> Option<usize> {
    change_start_rows(row_kinds)
        .into_iter()
        .find(|&start| start > row)
        .or_else(|| change_start_rows(row_kinds).first().copied())
}

/// The first row of the change before `row` (wrapping around), for Shift+F7.
pub fn previous_change_row(row_kinds: &[RowKind], row: usize) -> Option<usize> {
    let starts = change_start_rows(row_kinds);
    starts.iter().rev().find(|&&start| start < row).or(starts.last()).copied()
}

fn change_start_rows(row_kinds: &[RowKind]) -> Vec<usize> {
    (0..row_kinds.len())
        .filter(|&row| row_kinds[row] != RowKind::Unchanged && (row == 0 || row_kinds[row - 1] == RowKind::Unchanged))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::line_diff::{DiffOptions, diff_lines};
    use rustext_core::Text;

    fn line_diff(old: &str, new: &str) -> LineDiff {
        diff_lines(&Text::new(old), &Text::new(new), &DiffOptions::default())
    }

    #[test]
    fn side_by_side_pads_the_shorter_side() {
        let rows = side_by_side_rows(&line_diff("a\nb\nz\n", "a\nB1\nB2\nB3\nz\n"));

        let pairs: Vec<(Option<usize>, Option<usize>, RowKind)> =
            rows.iter().map(|row| (row.old_line, row.new_line, row.kind)).collect();
        assert_eq!(
            pairs,
            vec![
                (Some(0), Some(0), RowKind::Unchanged),
                (Some(1), Some(1), RowKind::Modified),
                (None, Some(2), RowKind::Added),
                (None, Some(3), RowKind::Added),
                (Some(2), Some(4), RowKind::Unchanged),
            ]
        );
    }

    #[test]
    fn inline_puts_removed_lines_before_added_ones() {
        let rows = inline_rows(&line_diff("a\nb\nc\n", "a\nB\nc\nd\n"));

        let kinds: Vec<RowKind> = rows.iter().map(|row| row.kind).collect();
        assert_eq!(
            kinds,
            vec![RowKind::Unchanged, RowKind::Removed, RowKind::Added, RowKind::Unchanged, RowKind::Added]
        );
        assert_eq!(rows[3].old_line, Some(2));
        assert_eq!(rows[3].new_line, Some(2));
    }

    #[test]
    fn collapses_far_unchanged_rows() {
        let mut kinds = vec![RowKind::Unchanged; 20];
        kinds[10] = RowKind::Modified;
        let options = CollapseOptions { context_rows: 2, minimum_collapsed_rows: 3 };

        let segments = collapse_unchanged(&kinds, &options, &[]);

        assert_eq!(
            segments,
            vec![
                Segment::Collapsed { start_row: 0, end_row: 8 },
                Segment::Shown { start_row: 8, end_row: 13 },
                Segment::Collapsed { start_row: 13, end_row: 20 },
            ]
        );
    }

    #[test]
    fn short_runs_and_expanded_runs_stay_shown() {
        let mut kinds = vec![RowKind::Unchanged; 12];
        kinds[0] = RowKind::Added;
        kinds[6] = RowKind::Added;
        let options = CollapseOptions { context_rows: 1, minimum_collapsed_rows: 4 };

        assert_eq!(
            collapse_unchanged(&kinds, &options, &[]),
            vec![Segment::Shown { start_row: 0, end_row: 8 }, Segment::Collapsed { start_row: 8, end_row: 12 }]
        );
        assert_eq!(collapse_unchanged(&kinds, &options, &[8]), vec![Segment::Shown { start_row: 0, end_row: 12 }]);
    }

    #[test]
    fn navigates_between_changes_and_wraps() {
        let kinds = [
            RowKind::Unchanged,
            RowKind::Added,
            RowKind::Added,
            RowKind::Unchanged,
            RowKind::Removed,
            RowKind::Unchanged,
        ];

        assert_eq!(next_change_row(&kinds, 0), Some(1));
        assert_eq!(next_change_row(&kinds, 1), Some(4));
        assert_eq!(next_change_row(&kinds, 4), Some(1));
        assert_eq!(previous_change_row(&kinds, 4), Some(1));
        assert_eq!(previous_change_row(&kinds, 1), Some(4));
    }
}
