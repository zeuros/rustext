//! A diff as editor rows and decorations: what each side of a side-by-side view, or the single
//! editor of an inline view, paints. Pure functions; the widget passes the results to its
//! code editors.

use rustext_core::decoration::{Decoration, DecorationKind};
use rustext_core::display::{DisplayLayout, DisplayRow, RowContent};
use rustext_core::style::Rgba;
use rustext_core::{Position, Text};

use crate::layout::{CollapseOptions, RowKind, Segment, collapse_unchanged, inline_rows, side_by_side_rows};
use crate::line_diff::LineDiff;

/// Colors of a diff, as data (the widget's theme provides them).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DiffColors {
    pub removed_line: Rgba,
    pub added_line: Rgba,
    pub removed_text: Rgba,
    pub added_text: Rgba,
}

/// How unchanged regions are shown: all of them, or collapsed except the ones the user opened
/// (identified by the old line they start at, stable while the new side is edited).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Collapsing {
    pub options: Option<CollapseOptions>,
    pub expanded_old_lines: Vec<usize>,
}

/// Where each side's lines wrap (`points[line]`: the start columns of its continuation rows,
/// as the editor showing it wraps them); empty when a side doesn't wrap.
#[derive(Clone, Copy, Debug, Default)]
pub struct WrapPoints<'a> {
    pub old: &'a [Vec<usize>],
    pub new: &'a [Vec<usize>],
}

impl<'a> WrapPoints<'a> {
    pub const NONE: WrapPoints<'static> = WrapPoints { old: &[], new: &[] };

    fn of(points: &'a [Vec<usize>], line: Option<usize>) -> &'a [usize] {
        line.and_then(|line| points.get(line)).map_or(&[], Vec::as_slice)
    }
}

/// The rows of a line on one side: its row, a row per continuation (same background), then
/// padding up to `height` rows (the other side's line wraps more).
fn push_line_rows(rows: &mut Vec<DisplayRow>, first: DisplayRow, line: Option<usize>, points: &[usize], height: usize) {
    let background = first.background;
    rows.push(first);
    if let Some(line) = line {
        rows.extend(points.iter().map(|&start_column| {
            let row = DisplayRow::new(RowContent::Wrapped { line, start_column });
            match background {
                Some(background) => row.with_background(background),
                None => row,
            }
        }));
    }
    let shown = 1 + if line.is_some() { points.len() } else { 0 };
    rows.extend((shown..height).map(|_| DisplayRow::new(RowContent::Padding)));
}

/// One row of either layout, before it becomes a `DisplayRow` for one side.
struct DiffRow {
    old_line: Option<usize>,
    new_line: Option<usize>,
    kind: RowKind,
}

/// Rows grouped into shown and collapsed segments, with each collapsed segment's lines on both
/// sides.
fn segments_of(rows: &[DiffRow], collapsing: &Collapsing) -> Vec<Segment> {
    let Some(options) = collapsing.options else {
        return vec![Segment::Shown { start_row: 0, end_row: rows.len() }];
    };
    let kinds: Vec<RowKind> = rows.iter().map(|row| row.kind).collect();
    let expanded_start_rows: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            row.kind == RowKind::Unchanged
                && row.old_line.is_some_and(|line| collapsing.expanded_old_lines.contains(&line))
        })
        .map(|(row, _)| row)
        .collect();
    let segments = collapse_unchanged(&kinds, &options, &[]);
    // A segment is expanded when the user opened the region starting at one of its rows.
    segments
        .into_iter()
        .map(|segment| match segment {
            Segment::Collapsed { start_row, end_row }
                if expanded_start_rows.iter().any(|row| (start_row..end_row).contains(row)) =>
            {
                Segment::Shown { start_row, end_row }
            }
            other => other,
        })
        .collect()
}

/// The display row of a diff row on one side, `None` for a side with no line.
fn side_row(line: Option<usize>, kind: RowKind, is_old_side: bool, colors: &DiffColors) -> DisplayRow {
    let Some(line) = line else { return DisplayRow::new(RowContent::Padding) };
    let row = DisplayRow::new(RowContent::Line(line));
    match (kind, is_old_side) {
        (RowKind::Unchanged, _) => row,
        (_, true) => row.with_background(colors.removed_line).with_marker('-'),
        (_, false) => row.with_background(colors.added_line).with_marker('+'),
    }
}

fn collapsed_row(rows: &[DiffRow], segment_rows: std::ops::Range<usize>, is_old_side: bool) -> DisplayRow {
    let line_of = |row: &DiffRow| if is_old_side { row.old_line } else { row.new_line };
    let first_line = rows[segment_rows.clone()].iter().find_map(line_of).unwrap_or(0);
    let last_line = rows[segment_rows].iter().rev().find_map(line_of).unwrap_or(first_line);
    DisplayRow::new(RowContent::Collapsed { first_line, last_line })
}

/// Rows of the old and new editors of a side-by-side view, aligned row for row: a line that
/// wraps into more rows than the line beside it gets padding beside its continuations.
pub fn side_by_side_layouts(
    diff: &LineDiff,
    old: &Text,
    new: &Text,
    collapsing: &Collapsing,
    colors: &DiffColors,
    wraps: &WrapPoints<'_>,
) -> (DisplayLayout, DisplayLayout) {
    let rows: Vec<DiffRow> = side_by_side_rows(diff)
        .into_iter()
        .map(|row| DiffRow { old_line: row.old_line, new_line: row.new_line, kind: row.kind })
        .collect();
    let mut old_rows = Vec::with_capacity(rows.len());
    let mut new_rows = Vec::with_capacity(rows.len());
    for segment in segments_of(&rows, collapsing) {
        match segment {
            Segment::Shown { start_row, end_row } => {
                for row in &rows[start_row..end_row] {
                    let old_points = WrapPoints::of(wraps.old, row.old_line);
                    let new_points = WrapPoints::of(wraps.new, row.new_line);
                    let height = 1 + old_points.len().max(new_points.len());
                    let old_first = side_row(row.old_line, row.kind, true, colors);
                    push_line_rows(&mut old_rows, old_first, row.old_line, old_points, height);
                    let new_first = side_row(row.new_line, row.kind, false, colors);
                    push_line_rows(&mut new_rows, new_first, row.new_line, new_points, height);
                }
            }
            Segment::Collapsed { start_row, end_row } => {
                old_rows.push(collapsed_row(&rows, start_row..end_row, true));
                new_rows.push(collapsed_row(&rows, start_row..end_row, false));
            }
        }
    }
    push_trailing_empty_line(&mut old_rows, old, diff.old_line_count);
    push_trailing_empty_line(&mut new_rows, new, diff.new_line_count);
    (DisplayLayout::new(old_rows, old.line_count()), DisplayLayout::new(new_rows, new.line_count()))
}

/// Rows of the new editor of an inline view: removed lines are foreign rows from the old text
/// (they don't wrap), the new text's lines wrap at `new_wrap_points`.
pub fn inline_layout(
    diff: &LineDiff,
    new: &Text,
    collapsing: &Collapsing,
    colors: &DiffColors,
    new_wrap_points: &[Vec<usize>],
) -> DisplayLayout {
    let rows: Vec<DiffRow> = inline_rows(diff)
        .into_iter()
        .map(|row| DiffRow { old_line: row.old_line, new_line: row.new_line, kind: row.kind })
        .collect();
    let mut display_rows = Vec::with_capacity(rows.len());
    for segment in segments_of(&rows, collapsing) {
        match segment {
            Segment::Shown { start_row, end_row } => {
                for row in &rows[start_row..end_row] {
                    match (row.kind, row.old_line, row.new_line) {
                        (RowKind::Removed, Some(old_line), _) => display_rows.push(
                            DisplayRow::new(RowContent::Foreign(old_line))
                                .with_background(colors.removed_line)
                                .with_marker('-'),
                        ),
                        (_, _, new_line) => {
                            let points = WrapPoints::of(new_wrap_points, new_line);
                            let first = side_row(new_line, row.kind, false, colors);
                            push_line_rows(&mut display_rows, first, new_line, points, 1 + points.len());
                        }
                    }
                }
            }
            Segment::Collapsed { start_row, end_row } => {
                display_rows.push(collapsed_row(&rows, start_row..end_row, false))
            }
        }
    }
    push_trailing_empty_line(&mut display_rows, new, diff.new_line_count);
    DisplayLayout::new(display_rows, new.line_count())
}

/// A text ending with a newline has an empty last line that git doesn't count: give it a row
/// so the cursor can go there.
fn push_trailing_empty_line(rows: &mut Vec<DisplayRow>, text: &Text, git_line_count: usize) {
    if text.line_count() > git_line_count {
        rows.push(DisplayRow::new(RowContent::Line(text.last_line())));
    }
}

/// Backgrounds of the changed words: on the old text, and on the new text.
pub fn inner_change_decorations(
    diff: &LineDiff,
    old: &Text,
    new: &Text,
    colors: &DiffColors,
) -> (Vec<Decoration>, Vec<Decoration>) {
    let decoration = |text: &Text, start: Position, end: Position, color: Rgba| {
        let end = Position::new(end.line, end.column.min(text.line_len(end.line)));
        Decoration::new(text.offset(start), text.offset(end), DecorationKind::Background(color))
    };
    let changes = diff.hunks.iter().flat_map(|hunk| hunk.inner_changes.iter());
    let old_decorations = changes
        .clone()
        .filter(|change| !change.old.is_empty())
        .map(|change| decoration(old, change.old.start, change.old.end, colors.removed_text))
        .collect();
    let new_decorations = changes
        .filter(|change| !change.new.is_empty())
        .map(|change| decoration(new, change.new.start, change.new.end, colors.added_text))
        .collect();
    (old_decorations, new_decorations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::line_diff::{DiffOptions, diff_lines};

    const COLORS: DiffColors = DiffColors {
        removed_line: Rgba::rgb(255, 0, 0),
        added_line: Rgba::rgb(0, 255, 0),
        removed_text: Rgba::rgb(128, 0, 0),
        added_text: Rgba::rgb(0, 128, 0),
    };

    fn contents(layout: &DisplayLayout) -> Vec<RowContent> {
        layout.rows().iter().map(|row| row.content).collect()
    }

    #[test]
    fn a_wrapped_line_gets_padding_beside_its_continuations() {
        let (old, new) = (Text::new("long line\nsame"), Text::new("short\nsame"));
        let diff = diff_lines(&old, &new, &DiffOptions::default());
        let old_points = vec![vec![5], vec![]];
        let wraps = WrapPoints { old: &old_points, new: &[] };

        let (old_layout, new_layout) = side_by_side_layouts(&diff, &old, &new, &Collapsing::default(), &COLORS, &wraps);

        assert_eq!(
            contents(&old_layout),
            vec![RowContent::Line(0), RowContent::Wrapped { line: 0, start_column: 5 }, RowContent::Line(1)]
        );
        assert_eq!(contents(&new_layout), vec![RowContent::Line(0), RowContent::Padding, RowContent::Line(1)]);
        assert_eq!(old_layout.rows()[1].background, Some(COLORS.removed_line));
    }

    #[test]
    fn side_by_side_sides_have_the_same_row_count() {
        let (old, new) = (Text::new("a\nb\nc\n"), Text::new("a\nB\nB2\nc\n"));
        let diff = diff_lines(&old, &new, &DiffOptions::default());

        let (old_layout, new_layout) =
            side_by_side_layouts(&diff, &old, &new, &Collapsing::default(), &COLORS, &WrapPoints::NONE);

        assert_eq!(
            contents(&old_layout),
            vec![
                RowContent::Line(0),
                RowContent::Line(1),
                RowContent::Padding,
                RowContent::Line(2),
                RowContent::Line(3)
            ]
        );
        assert_eq!(old_layout.row_count(), new_layout.row_count());
        assert_eq!(new_layout.rows()[2].marker, Some('+'));
    }

    #[test]
    fn inline_shows_removed_lines_as_foreign_rows() {
        let (old, new) = (Text::new("a\nb\n"), Text::new("a\nc\n"));
        let diff = diff_lines(&old, &new, &DiffOptions::default());

        let layout = inline_layout(&diff, &new, &Collapsing::default(), &COLORS, &[]);

        assert_eq!(
            contents(&layout),
            vec![RowContent::Line(0), RowContent::Foreign(1), RowContent::Line(1), RowContent::Line(2)]
        );
    }

    #[test]
    fn collapsed_regions_become_one_row_per_side_until_expanded() {
        let old_content: String = (0..20).map(|line| format!("line {line}\n")).collect();
        let new_content = old_content.replace("line 10\n", "changed\n");
        let (old, new) = (Text::new(&old_content), Text::new(&new_content));
        let diff = diff_lines(&old, &new, &DiffOptions::default());
        let options = Some(CollapseOptions { context_rows: 1, minimum_collapsed_rows: 3 });

        let (old_layout, _) = side_by_side_layouts(
            &diff,
            &old,
            &new,
            &Collapsing { options, expanded_old_lines: vec![] },
            &COLORS,
            &WrapPoints::NONE,
        );
        assert_eq!(old_layout.rows()[0].content, RowContent::Collapsed { first_line: 0, last_line: 8 });
        assert_eq!(old_layout.row_count(), 1 + 3 + 1 + 1);

        let expanded = Collapsing { options, expanded_old_lines: vec![0] };
        let (old_layout, _) = side_by_side_layouts(&diff, &old, &new, &expanded, &COLORS, &WrapPoints::NONE);
        assert_eq!(old_layout.rows()[0].content, RowContent::Line(0));
    }

    #[test]
    fn inner_changes_become_backgrounds_on_both_texts() {
        let (old, new) = (Text::new("let a = 1;\n"), Text::new("let b = 1;\n"));
        let diff = diff_lines(&old, &new, &DiffOptions::default());

        let (old_decorations, new_decorations) = inner_change_decorations(&diff, &old, &new, &COLORS);

        assert_eq!((old_decorations[0].start, old_decorations[0].end), (4, 5));
        assert_eq!((new_decorations[0].start, new_decorations[0].end), (4, 5));
    }
}
