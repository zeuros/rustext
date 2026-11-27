//! Moved code (Monaco's `experimental.showMoves`, DECISIONS D30): a block of lines removed in
//! one hunk and added in another, compared without their indentation, is shown as moved
//! rather than as unrelated changes.

use std::collections::HashMap;

use rustext_core::Text;

use crate::line_diff::{Hunk, LineRange};

/// Lines removed at `old` and added, the same but for their indentation, at `new`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MovedBlock {
    pub old: LineRange,
    pub new: LineRange,
}

/// Fewer lines than this are not a move: a `}` and a blank line match too easily.
pub const MIN_MOVED_LINES: usize = 3;

/// A move holds at least this many chars that aren't whitespace.
const MIN_MOVED_CHARS: usize = 20;

/// Added lines tried as the start of a move for one removed line; past this the line is too
/// common to tell where it went.
const MAX_CANDIDATES_PER_LINE: usize = 64;

/// The added line where each removed line could have gone, by its trimmed content; blank lines
/// start no move.
fn added_lines_by_content<'text>(new: &'text [String], hunks: &[Hunk]) -> HashMap<&'text str, Vec<usize>> {
    let mut lines: HashMap<&str, Vec<usize>> = HashMap::new();
    for line in hunks.iter().flat_map(|hunk| hunk.new.iter()) {
        let content = new[line].as_str();
        if !content.is_empty() {
            lines.entry(content).or_default().push(line);
        }
    }
    lines
}

/// The trimmed content of each line of `text`.
fn trimmed_lines(text: &Text) -> Vec<String> {
    (0..text.line_count()).map(|line| text.line_content(line).trim().to_string()).collect()
}

/// The moved blocks between the hunks of a diff of `old` and `new`: going down the removed
/// lines, each takes the longest run of equal added lines in another hunk that no earlier
/// block took.
pub fn detect_moves(old: &Text, new: &Text, hunks: &[Hunk]) -> Vec<MovedBlock> {
    let (old_lines, new_lines) = (trimmed_lines(old), trimmed_lines(new));
    let candidates = added_lines_by_content(&new_lines, hunks);
    let new_hunk_of: HashMap<usize, usize> =
        hunks.iter().enumerate().flat_map(|(index, hunk)| hunk.new.iter().map(move |line| (line, index))).collect();
    let mut is_taken = vec![false; new_lines.len()];
    let mut moves = Vec::new();

    for (hunk_index, hunk) in hunks.iter().enumerate() {
        let mut old_line = hunk.old.start;
        while old_line < hunk.old.end {
            let run_length = |new_start: usize| {
                let new_end = hunks[new_hunk_of[&new_start]].new.end;
                (0..)
                    .take_while(|&offset| {
                        let (old_at, new_at) = (old_line + offset, new_start + offset);
                        old_at < hunk.old.end
                            && new_at < new_end
                            && !is_taken[new_at]
                            && old_lines[old_at] == new_lines[new_at]
                    })
                    .count()
            };
            let longest = candidates
                .get(old_lines[old_line].as_str())
                .into_iter()
                .flatten()
                .take(MAX_CANDIDATES_PER_LINE)
                .filter(|new_start| new_hunk_of[new_start] != hunk_index && !is_taken[**new_start])
                .map(|&new_start| (new_start, run_length(new_start)))
                .max_by_key(|&(new_start, length)| (length, std::cmp::Reverse(new_start)));
            let moved = longest.filter(|&(_, length)| {
                let chars: usize = old_lines[old_line..old_line + length]
                    .iter()
                    .map(|content| content.chars().filter(|character| !character.is_whitespace()).count())
                    .sum();
                length >= MIN_MOVED_LINES && chars >= MIN_MOVED_CHARS
            });
            let Some((new_start, length)) = moved else {
                old_line += 1;
                continue;
            };
            is_taken[new_start..new_start + length].fill(true);
            moves.push(MovedBlock {
                old: LineRange::new(old_line, old_line + length),
                new: LineRange::new(new_start, new_start + length),
            });
            old_line += length;
        }
    }
    moves
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::line_diff::{DiffOptions, diff_lines};

    fn moves_of(old: &str, new: &str) -> Vec<(LineRange, LineRange)> {
        let (old, new) = (Text::new(old), Text::new(new));
        let diff = diff_lines(&old, &new, &DiffOptions { detect_moves: true, ..DiffOptions::default() });
        diff.moves.iter().map(|moved| (moved.old, moved.new)).collect()
    }

    const FUNCTION: &str = "fn area(width: u32, height: u32) -> u32 {\n    width * height\n}\n";
    const OTHER: &str =
        "fn perimeter(width: u32, height: u32) -> u32 {\n    let sum = width + height;\n    2 * sum\n}\n";

    #[test]
    fn a_function_moved_below_another_is_a_move() {
        let old = format!("{FUNCTION}\n{OTHER}");
        let new = format!("{OTHER}\n{FUNCTION}");

        assert_eq!(moves_of(&old, &new), vec![(LineRange::new(0, 3), LineRange::new(5, 8))]);
    }

    #[test]
    fn a_moved_block_reindented_is_still_a_move() {
        let indented = FUNCTION.lines().map(|line| format!("    {line}\n")).collect::<String>();
        let old = format!("{FUNCTION}mod shapes {{\n}}\n");
        let new = format!("mod shapes {{\n{indented}}}\n");

        assert_eq!(moves_of(&old, &new), vec![(LineRange::new(0, 3), LineRange::new(1, 4))]);
    }

    #[test]
    fn short_blocks_are_not_moves() {
        let old = "}\n\nlet a = 1;\nx\n";
        let new = "x\n}\n\nlet a = 1;\n";

        assert_eq!(moves_of(old, new), vec![]);
    }

    #[test]
    fn lines_changed_in_place_are_not_a_move() {
        let old = format!("{FUNCTION}tail\n");
        let new = FUNCTION.replace("width * height", "height * width") + "tail\n";

        assert_eq!(moves_of(&old, &new), vec![]);
    }

    #[test]
    fn moves_are_off_by_default() {
        let old = Text::new(&format!("{FUNCTION}\n{OTHER}"));
        let new = Text::new(&format!("{OTHER}\n{FUNCTION}"));

        assert!(diff_lines(&old, &new, &DiffOptions::default()).moves.is_empty());
    }
}
