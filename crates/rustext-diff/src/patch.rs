//! Patches of selected lines, for staging, unstaging and discarding parts of a diff with
//! `git apply --unidiff-zero` (gitgud's "stage this line / hunk").
//!
//! One rule covers every case (pure insertions, deletions, replacements of any size):
//! - `Stage` (applied forward to the old side, `git apply --cached`): selected removed and added
//!   lines are kept; an unselected removed line becomes context (it stays); an unselected added
//!   line is left out (it stays unstaged).
//! - `Revert` (applied in reverse to the new side, `git apply -R [--cached]`, to unstage or
//!   discard): selected lines are kept; an unselected added line becomes context (it stays);
//!   an unselected removed line is left out.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use rustext_core::Text;

use crate::line_diff::{LineDiff, raw_line};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PatchDirection {
    /// Apply forward to the old text: `git apply --cached --unidiff-zero`.
    Stage,
    /// Apply in reverse to the new text: `git apply -R [--cached] --unidiff-zero`.
    Revert,
}

/// Lines picked by the user: lines of the old text (removed lines) and of the new text (added
/// lines), 0-based. Unchanged lines in it are ignored.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineSelection {
    pub old_lines: BTreeSet<usize>,
    pub new_lines: BTreeSet<usize>,
}

impl LineSelection {
    /// Every changed line of hunk `hunk_index`.
    pub fn hunk(diff: &LineDiff, hunk_index: usize) -> Self {
        let hunk = &diff.hunks[hunk_index];
        Self { old_lines: hunk.old.iter().collect(), new_lines: hunk.new.iter().collect() }
    }

    /// Every changed line.
    pub fn all(diff: &LineDiff) -> Self {
        Self {
            old_lines: diff.hunks.iter().flat_map(|hunk| hunk.old.iter()).collect(),
            new_lines: diff.hunks.iter().flat_map(|hunk| hunk.new.iter()).collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.old_lines.is_empty() && self.new_lines.is_empty()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Context,
    Removed,
    Added,
}

/// The patch for the selected lines, with zero context lines, or `None` when the selection
/// holds no changed line.
#[expect(clippy::too_many_arguments, reason = "predates the complexity lints (D28)")]
pub fn build_patch(
    diff: &LineDiff,
    old: &Text,
    new: &Text,
    selection: &LineSelection,
    direction: PatchDirection,
    file_path: &str,
) -> Option<String> {
    let mut hunks_text = String::new();
    // Lines added minus lines removed by the hunks emitted so far, which shifts the side the
    // patch is not applied to.
    let mut emitted_line_delta: isize = 0;

    for hunk in &diff.hunks {
        let removed_selected: Vec<usize> = hunk.old.iter().filter(|line| selection.old_lines.contains(line)).collect();
        let added_selected: Vec<usize> = hunk.new.iter().filter(|line| selection.new_lines.contains(line)).collect();
        if removed_selected.is_empty() && added_selected.is_empty() {
            continue;
        }

        let mut lines: Vec<(Mark, String)> = Vec::new();
        match direction {
            PatchDirection::Stage => {
                for line in hunk.old.iter() {
                    let mark = if selection.old_lines.contains(&line) { Mark::Removed } else { Mark::Context };
                    lines.push((mark, raw_line(old, line)));
                }
                lines.extend(added_selected.iter().map(|&line| (Mark::Added, raw_line(new, line))));
            }
            PatchDirection::Revert => {
                lines.extend(removed_selected.iter().map(|&line| (Mark::Removed, raw_line(old, line))));
                for line in hunk.new.iter() {
                    let mark = if selection.new_lines.contains(&line) { Mark::Added } else { Mark::Context };
                    lines.push((mark, raw_line(new, line)));
                }
            }
        }

        let old_count = lines.iter().filter(|(mark, _)| *mark != Mark::Added).count();
        let new_count = lines.iter().filter(|(mark, _)| *mark != Mark::Removed).count();
        // 0-based line before which the hunk's lines sit, on each side.
        let (old_position, new_position) = match direction {
            PatchDirection::Stage => (hunk.old.start, hunk.old.start.saturating_add_signed(emitted_line_delta)),
            PatchDirection::Revert => (hunk.new.start.saturating_add_signed(-emitted_line_delta), hunk.new.start),
        };
        emitted_line_delta += new_count as isize - old_count as isize;

        let _ = writeln!(
            hunks_text,
            "@@ -{} +{} @@",
            header_range(old_position, old_count),
            header_range(new_position, new_count)
        );
        for (mark, content) in lines {
            let prefix = match mark {
                Mark::Context => ' ',
                Mark::Removed => '-',
                Mark::Added => '+',
            };
            hunks_text.push(prefix);
            hunks_text.push_str(&content);
            if !content.ends_with('\n') {
                hunks_text.push_str("\n\\ No newline at end of file\n");
            }
        }
    }

    (!hunks_text.is_empty()).then(|| format!("--- a/{file_path}\n+++ b/{file_path}\n{hunks_text}"))
}

/// `start,count` of a hunk header, 1-based. An empty range names the line it follows.
fn header_range(position: usize, count: usize) -> String {
    match count {
        0 => format!("{position},0"),
        1 => format!("{}", position + 1),
        _ => format!("{},{count}", position + 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::line_diff::{DiffOptions, diff_lines};

    fn patch(old: &str, new: &str, selection: LineSelection, direction: PatchDirection) -> Option<String> {
        let (old, new) = (Text::new(old), Text::new(new));
        let diff = diff_lines(&old, &new, &DiffOptions::default());
        build_patch(&diff, &old, &new, &selection, direction, "f.txt")
    }

    fn new_lines(lines: &[usize]) -> LineSelection {
        LineSelection { new_lines: lines.iter().copied().collect(), ..LineSelection::default() }
    }

    fn old_lines(lines: &[usize]) -> LineSelection {
        LineSelection { old_lines: lines.iter().copied().collect(), ..LineSelection::default() }
    }

    #[test]
    fn stages_one_inserted_line() {
        let patch = patch("a\nb\n", "a\nx\ny\nb\n", new_lines(&[2]), PatchDirection::Stage).unwrap();

        assert_eq!(patch, "--- a/f.txt\n+++ b/f.txt\n@@ -1,0 +2 @@\n+y\n");
    }

    #[test]
    fn staging_part_of_a_replacement_keeps_unselected_removed_lines() {
        let patch = patch(
            "a\nold1\nold2\nb\n",
            "a\nnew1\nb\n",
            LineSelection { old_lines: [1].into(), new_lines: [1].into() },
            PatchDirection::Stage,
        );

        assert_eq!(patch.unwrap(), "--- a/f.txt\n+++ b/f.txt\n@@ -2,2 +2,2 @@\n-old1\n old2\n+new1\n");
    }

    #[test]
    fn stages_a_deletion() {
        let patch = patch("a\ngone\nb\n", "a\nb\n", old_lines(&[1]), PatchDirection::Stage).unwrap();

        assert_eq!(patch, "--- a/f.txt\n+++ b/f.txt\n@@ -2 +1,0 @@\n-gone\n");
    }

    #[test]
    fn later_hunks_account_for_lines_staged_before_them() {
        let patch = patch("a\nb\nc\n", "x\na\nb\nc\ny\n", new_lines(&[0, 4]), PatchDirection::Stage).unwrap();

        assert_eq!(patch, "--- a/f.txt\n+++ b/f.txt\n@@ -0,0 +1 @@\n+x\n@@ -3,0 +5 @@\n+y\n");
    }

    #[test]
    fn reverting_an_added_line_keeps_the_other_added_lines_as_context() {
        let patch = patch("a\nb\n", "a\nx\ny\nb\n", new_lines(&[1]), PatchDirection::Revert).unwrap();

        assert_eq!(patch, "--- a/f.txt\n+++ b/f.txt\n@@ -2 +2,2 @@\n+x\n y\n");
    }

    #[test]
    fn missing_final_newline_gets_the_marker() {
        let patch = patch("a\n", "a\nb", new_lines(&[1]), PatchDirection::Stage).unwrap();

        assert_eq!(patch, "--- a/f.txt\n+++ b/f.txt\n@@ -1,0 +2 @@\n+b\n\\ No newline at end of file\n");
    }

    #[test]
    fn selection_outside_changes_gives_no_patch() {
        assert_eq!(patch("a\nb\n", "a\nc\n", old_lines(&[0]), PatchDirection::Stage), None);
    }
}
