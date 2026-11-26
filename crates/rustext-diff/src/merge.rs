//! Three-way merge (diff3): the changes of "ours" and "theirs" against their common "base",
//! grouped into regions; regions changed on both sides differently are conflicts.

use rustext_core::Text;

use crate::line_diff::{DiffOptions, Hunk, LineRange, diff_lines, git_line_count, raw_line};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RegionKind {
    Unchanged,
    /// Only ours changed it.
    Ours,
    /// Only theirs changed it.
    Theirs,
    /// Both changed it the same way.
    Both,
    /// Both changed it differently.
    Conflict,
}

/// Lines of the base and the matching lines of each side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MergeRegion {
    pub base: LineRange,
    pub ours: LineRange,
    pub theirs: LineRange,
    pub kind: RegionKind,
}

/// What a region becomes in the result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Resolution {
    Base,
    Ours,
    Theirs,
    OursThenTheirs,
    TheirsThenOurs,
}

impl RegionKind {
    /// Non-conflicting changes are taken; a conflict keeps the base until someone decides.
    pub fn default_resolution(self) -> Resolution {
        match self {
            RegionKind::Unchanged | RegionKind::Conflict => Resolution::Base,
            RegionKind::Ours | RegionKind::Both => Resolution::Ours,
            RegionKind::Theirs => Resolution::Theirs,
        }
    }
}

/// The three texts of a merge.
#[derive(Clone, Debug)]
pub struct MergeInput {
    pub base: Text,
    pub ours: Text,
    pub theirs: Text,
}

/// Sum of the line count changes of `hunks` (lines added minus lines removed).
fn line_delta(hunks: &[&Hunk]) -> isize {
    hunks.iter().map(|hunk| hunk.new.len() as isize - hunk.old.len() as isize).sum()
}

fn shifted(line: usize, delta: isize) -> usize {
    line.saturating_add_signed(delta)
}

/// The regions of a merge, covering the whole base in order.
pub fn merge_regions(input: &MergeInput) -> Vec<MergeRegion> {
    let options = DiffOptions { inner_changes: false, ..DiffOptions::default() };
    let ours_diff = diff_lines(&input.base, &input.ours, &options);
    let theirs_diff = diff_lines(&input.base, &input.theirs, &options);

    // Every hunk, tagged with its side, sorted by where it starts in the base.
    let mut hunks: Vec<(bool, &Hunk)> = ours_diff
        .hunks
        .iter()
        .map(|hunk| (true, hunk))
        .chain(theirs_diff.hunks.iter().map(|hunk| (false, hunk)))
        .collect();
    hunks.sort_by_key(|(_, hunk)| (hunk.old.start, hunk.old.end));

    // Groups of hunks that overlap or touch in the base.
    let mut groups: Vec<(LineRange, Vec<(bool, &Hunk)>)> = Vec::new();
    for (is_ours, hunk) in hunks {
        match groups.last_mut() {
            Some((range, members)) if hunk.old.start <= range.end => {
                range.end = range.end.max(hunk.old.end);
                members.push((is_ours, hunk));
            }
            _ => groups.push((hunk.old, vec![(is_ours, hunk)])),
        }
    }

    let mut regions = Vec::new();
    let (mut ours_hunks_before, mut theirs_hunks_before): (Vec<&Hunk>, Vec<&Hunk>) = (Vec::new(), Vec::new());
    let mut base_line = 0;
    let unchanged = |base: LineRange, ours_delta: isize, theirs_delta: isize| MergeRegion {
        base,
        ours: LineRange::new(shifted(base.start, ours_delta), shifted(base.end, ours_delta)),
        theirs: LineRange::new(shifted(base.start, theirs_delta), shifted(base.end, theirs_delta)),
        kind: RegionKind::Unchanged,
    };
    for (base_range, members) in groups {
        let (ours_delta, theirs_delta) = (line_delta(&ours_hunks_before), line_delta(&theirs_hunks_before));
        if base_line < base_range.start {
            regions.push(unchanged(LineRange::new(base_line, base_range.start), ours_delta, theirs_delta));
        }
        let ours_members: Vec<&Hunk> = members.iter().filter(|(is_ours, _)| *is_ours).map(|(_, hunk)| *hunk).collect();
        let theirs_members: Vec<&Hunk> =
            members.iter().filter(|(is_ours, _)| !*is_ours).map(|(_, hunk)| *hunk).collect();
        let ours = LineRange::new(
            shifted(base_range.start, ours_delta),
            shifted(base_range.end, ours_delta + line_delta(&ours_members)),
        );
        let theirs = LineRange::new(
            shifted(base_range.start, theirs_delta),
            shifted(base_range.end, theirs_delta + line_delta(&theirs_members)),
        );
        let kind = match (ours_members.is_empty(), theirs_members.is_empty()) {
            (false, true) => RegionKind::Ours,
            (true, false) => RegionKind::Theirs,
            _ if lines(&input.ours, ours) == lines(&input.theirs, theirs) => RegionKind::Both,
            _ => RegionKind::Conflict,
        };
        regions.push(MergeRegion { base: base_range, ours, theirs, kind });
        ours_hunks_before.extend(ours_members);
        theirs_hunks_before.extend(theirs_members);
        base_line = base_range.end;
    }
    let base_line_count = git_line_count(&input.base);
    if base_line < base_line_count || regions.is_empty() {
        let (ours_delta, theirs_delta) = (line_delta(&ours_hunks_before), line_delta(&theirs_hunks_before));
        regions.push(unchanged(LineRange::new(base_line, base_line_count), ours_delta, theirs_delta));
    }
    regions
}

/// The raw lines (with line endings) of a range of a text.
pub fn lines(text: &Text, range: LineRange) -> String {
    range.iter().map(|line| raw_line(text, line)).collect()
}

/// What a region becomes with a resolution.
pub fn resolved_region(input: &MergeInput, region: &MergeRegion, resolution: Resolution) -> String {
    let (ours, theirs) = (lines(&input.ours, region.ours), lines(&input.theirs, region.theirs));
    match resolution {
        Resolution::Base => lines(&input.base, region.base),
        Resolution::Ours => ours,
        Resolution::Theirs => theirs,
        Resolution::OursThenTheirs => with_line_break_between(ours, theirs),
        Resolution::TheirsThenOurs => with_line_break_between(theirs, ours),
    }
}

/// `first` then `second`, with a line break between them if `first` lacks one (a last line).
fn with_line_break_between(mut first: String, second: String) -> String {
    if !first.is_empty() && !second.is_empty() && !first.ends_with('\n') {
        first.push('\n');
    }
    first + &second
}

/// The merged text, one resolution per region.
pub fn merged_text(input: &MergeInput, regions: &[MergeRegion], resolutions: &[Resolution]) -> String {
    regions.iter().zip(resolutions).map(|(region, &resolution)| resolved_region(input, region, resolution)).collect()
}

/// The merged text with git's conflict markers around unresolved conflicts, as
/// `git merge` writes it into the working tree.
pub fn text_with_conflict_markers(
    input: &MergeInput,
    regions: &[MergeRegion],
    ours_label: &str,
    theirs_label: &str,
) -> String {
    regions
        .iter()
        .map(|region| match region.kind {
            RegionKind::Conflict => {
                let ours = with_final_line_break(lines(&input.ours, region.ours));
                let theirs = with_final_line_break(lines(&input.theirs, region.theirs));
                format!("<<<<<<< {ours_label}\n{ours}=======\n{theirs}>>>>>>> {theirs_label}\n")
            }
            kind => resolved_region(input, region, kind.default_resolution()),
        })
        .collect()
}

fn with_final_line_break(mut lines: String) -> String {
    if !lines.is_empty() && !lines.ends_with('\n') {
        lines.push('\n');
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(base: &str, ours: &str, theirs: &str) -> MergeInput {
        MergeInput { base: Text::new(base), ours: Text::new(ours), theirs: Text::new(theirs) }
    }

    fn kinds(regions: &[MergeRegion]) -> Vec<RegionKind> {
        regions.iter().map(|region| region.kind).collect()
    }

    fn default_merge(input: &MergeInput) -> String {
        let regions = merge_regions(input);
        let resolutions: Vec<Resolution> = regions.iter().map(|region| region.kind.default_resolution()).collect();
        merged_text(input, &regions, &resolutions)
    }

    #[test]
    fn changes_on_different_lines_merge_cleanly() {
        let input = input("a\nb\nc\nd\ne\n", "A\nb\nc\nd\ne\n", "a\nb\nc\nd\nE\n");

        let regions = merge_regions(&input);

        assert_eq!(kinds(&regions), vec![RegionKind::Ours, RegionKind::Unchanged, RegionKind::Theirs]);
        assert_eq!(default_merge(&input), "A\nb\nc\nd\nE\n");
    }

    #[test]
    fn different_changes_to_the_same_line_conflict() {
        let input = input("a\nb\nc\n", "a\nours\nc\n", "a\ntheirs\nc\n");

        let regions = merge_regions(&input);

        assert_eq!(kinds(&regions), vec![RegionKind::Unchanged, RegionKind::Conflict, RegionKind::Unchanged]);
        assert_eq!(default_merge(&input), "a\nb\nc\n");
        let conflict = regions[1];
        assert_eq!(resolved_region(&input, &conflict, Resolution::OursThenTheirs), "ours\ntheirs\n");
    }

    #[test]
    fn the_same_change_on_both_sides_is_not_a_conflict() {
        let input = input("a\nb\n", "a\nB\n", "a\nB\n");

        assert_eq!(kinds(&merge_regions(&input)), vec![RegionKind::Unchanged, RegionKind::Both]);
        assert_eq!(default_merge(&input), "a\nB\n");
    }

    #[test]
    fn touching_changes_conflict_like_git() {
        let input = input("a\nb\nc\n", "a\nB\nc\n", "a\nb\nC\n");

        assert_eq!(kinds(&merge_regions(&input)), vec![RegionKind::Unchanged, RegionKind::Conflict]);
    }

    #[test]
    fn side_ranges_follow_insertions_before_them() {
        let input = input("a\nb\nc\nd\n", "new1\nnew2\na\nb\nc\nd\n", "a\nb\nc\nD\n");

        let regions = merge_regions(&input);

        let theirs_region = regions.iter().find(|region| region.kind == RegionKind::Theirs).unwrap();
        assert_eq!(theirs_region.base, LineRange::new(3, 4));
        assert_eq!(theirs_region.ours, LineRange::new(5, 6));
        assert_eq!(theirs_region.theirs, LineRange::new(3, 4));
        assert_eq!(default_merge(&input), "new1\nnew2\na\nb\nc\nD\n");
    }

    #[test]
    fn conflict_markers_like_git_writes_them() {
        let input = input("x\n", "ours\n", "theirs\n");
        let regions = merge_regions(&input);

        let marked = text_with_conflict_markers(&input, &regions, "HEAD", "feature");

        assert_eq!(marked, "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> feature\n");
    }

    #[test]
    fn identical_texts_are_one_unchanged_region() {
        let input = input("a\nb\n", "a\nb\n", "a\nb\n");

        assert_eq!(kinds(&merge_regions(&input)), vec![RegionKind::Unchanged]);
        assert_eq!(default_merge(&input), "a\nb\n");
    }
}
