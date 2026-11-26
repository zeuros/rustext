//! Random text pairs and triples, checking what diffs and merges must always satisfy: the row
//! layouts rebuild both texts, and a merge resolved all-ours or all-theirs gives that side back.

use rustext_core::Text;
use rustext_diff::line_diff::{git_line_count, raw_line};
use rustext_diff::merge::{MergeInput, Resolution, merge_regions, merged_text};
use rustext_diff::{DiffOptions, diff_lines, inline_rows, side_by_side_rows};

struct Random(u64);

impl Random {
    fn next(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % bound.max(1) as u64) as usize
    }
}

const LINES: &[&str] = &["a\n", "b\n", "c\n", "a\r\n", "", "fn x() {\n", "}\n", "\n", "👍🏽\n", "tail"];

fn random_text(random: &mut Random) -> Text {
    let mut content: String = (0..random.next(12)).map(|_| LINES[random.next(LINES.len() - 1)]).collect();
    if random.next(4) == 0 {
        content.push_str("no final newline");
    }
    Text::new(&content)
}

/// A variant of `base`: some of its lines dropped, replaced or added.
fn edited(base: &Text, random: &mut Random) -> Text {
    let mut lines: Vec<String> = (0..git_line_count(base)).map(|line| raw_line(base, line)).collect();
    for _ in 0..random.next(4) {
        let position = random.next(lines.len() + 1);
        match random.next(3) {
            0 if position < lines.len() => {
                lines.remove(position);
            }
            1 if position < lines.len() => lines[position] = LINES[random.next(LINES.len())].to_string(),
            _ => lines.insert(position, LINES[random.next(LINES.len() - 1)].to_string()),
        }
    }
    // Only a last line may lack its newline.
    let line_count = lines.len();
    for line in lines.iter_mut().take(line_count.saturating_sub(1)) {
        if !line.ends_with('\n') {
            line.push('\n');
        }
    }
    Text::new(&lines.concat())
}

fn raw(text: &Text, line: usize) -> String {
    raw_line(text, line)
}

#[test]
fn row_layouts_rebuild_both_texts() {
    let mut random = Random(0x5eed_1234_abcd_ef01);
    for case in 0..2_000 {
        let old = random_text(&mut random);
        let new = edited(&old, &mut random);
        let diff = diff_lines(&old, &new, &DiffOptions::default());

        let side_by_side = side_by_side_rows(&diff);
        let old_from_rows: String =
            side_by_side.iter().filter_map(|row| row.old_line).map(|line| raw(&old, line)).collect();
        let new_from_rows: String =
            side_by_side.iter().filter_map(|row| row.new_line).map(|line| raw(&new, line)).collect();
        assert_eq!(old_from_rows, old.to_string(), "case {case}: side-by-side old side");
        assert_eq!(new_from_rows, new.to_string(), "case {case}: side-by-side new side");

        let inline = inline_rows(&diff);
        let old_from_inline: String =
            inline.iter().filter_map(|row| row.old_line).map(|line| raw(&old, line)).collect();
        let new_from_inline: String =
            inline.iter().filter_map(|row| row.new_line).map(|line| raw(&new, line)).collect();
        assert_eq!(old_from_inline, old.to_string(), "case {case}: inline old lines");
        assert_eq!(new_from_inline, new.to_string(), "case {case}: inline new lines");

        // Applying every hunk to the old text gives the new one.
        let mut rebuilt = String::new();
        let mut old_line = 0;
        for hunk in &diff.hunks {
            rebuilt.extend((old_line..hunk.old.start).map(|line| raw(&old, line)));
            rebuilt.extend(hunk.new.iter().map(|line| raw(&new, line)));
            old_line = hunk.old.end;
        }
        rebuilt.extend((old_line..git_line_count(&old)).map(|line| raw(&old, line)));
        assert_eq!(rebuilt, new.to_string(), "case {case}: hunks");
    }
}

#[test]
fn merges_resolved_to_one_side_give_that_side() {
    let mut random = Random(0x0ddb_a11c_0ffe_e123);
    for case in 0..2_000 {
        let base = random_text(&mut random);
        let input = MergeInput { ours: edited(&base, &mut random), theirs: edited(&base, &mut random), base };
        let regions = merge_regions(&input);

        for (resolution, side) in
            [(Resolution::Ours, &input.ours), (Resolution::Theirs, &input.theirs), (Resolution::Base, &input.base)]
        {
            let resolutions = vec![resolution; regions.len()];
            assert_eq!(
                merged_text(&input, &regions, &resolutions),
                side.to_string(),
                "case {case}: all {resolution:?}"
            );
        }
    }
}
