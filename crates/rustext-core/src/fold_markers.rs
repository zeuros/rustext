//! Folding regions marked in comments (Monaco's `folding.markers`, DECISIONS D33): `// #region`
//! … `// #endregion`, in any language, whatever the folding strategy.

use crate::structure::FoldRange;
use crate::text::Text;

/// What a line marks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionMarker {
    Start,
    End,
}

/// Comment openings a marker may follow; `#` is one too (Python, shell, C#'s `#region`).
const COMMENT_STARTS: &[&str] = &["//", "/*", "<!--", "--", ";", "%", "'"];

/// The marker `line_content` is, if any: a comment (or a `#` line) whose first word is
/// `#region`, `region`, `region:` (`// region: name`, rust-analyzer's), or their `endregion`.
pub fn region_marker(line_content: &str) -> Option<RegionMarker> {
    let trimmed = line_content.trim_start();
    let after_comment = COMMENT_STARTS
        .iter()
        .find_map(|start| trimmed.strip_prefix(start))
        .map(|rest| rest.trim_start().strip_prefix('#').unwrap_or(rest.trim_start()))
        .or_else(|| trimmed.strip_prefix('#').map(str::trim_start))?;
    let word_end =
        after_comment.find(|character: char| !character.is_ascii_alphanumeric()).unwrap_or(after_comment.len());
    match &after_comment[..word_end] {
        "region" => Some(RegionMarker::Start),
        "endregion" => Some(RegionMarker::End),
        _ => None,
    }
}

/// The region `header_line` starts: up to its end marker, nested regions skipped. The end
/// marker's line is hidden with it, as in VS Code. `None` without an end marker.
pub fn region_fold_range(text: &Text, header_line: usize) -> Option<FoldRange> {
    if region_marker(&text.line_content(header_line)) != Some(RegionMarker::Start) {
        return None;
    }
    let mut depth = 0usize;
    for line in header_line + 1..text.line_count() {
        match region_marker(&text.line_content(line)) {
            Some(RegionMarker::Start) => depth += 1,
            Some(RegionMarker::End) if depth == 0 => {
                return Some(FoldRange { header_line, last_hidden_line: line });
            }
            Some(RegionMarker::End) => depth -= 1,
            None => {}
        }
    }
    None
}

/// Every region of the text, for fold all.
pub fn region_fold_ranges(text: &Text) -> Vec<FoldRange> {
    let mut open_headers = Vec::new();
    let mut ranges = Vec::new();
    for line in 0..text.line_count() {
        match region_marker(&text.line_content(line)) {
            Some(RegionMarker::Start) => open_headers.push(line),
            Some(RegionMarker::End) => {
                if let Some(header_line) = open_headers.pop() {
                    ranges.push(FoldRange { header_line, last_hidden_line: line });
                }
            }
            None => {}
        }
    }
    ranges.sort_by_key(|range| range.header_line);
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_of_several_languages_are_recognized() {
        for start in [
            "// #region setup",
            "//#region",
            "# region",
            "#region Fields",
            "/* #region */",
            "<!-- #region -->",
            "// region: helpers",
        ] {
            assert_eq!(region_marker(start), Some(RegionMarker::Start), "{start}");
        }
        for end in ["// #endregion", "#endregion", "<!-- #endregion -->", "// endregion"] {
            assert_eq!(region_marker(end), Some(RegionMarker::End), "{end}");
        }
    }

    #[test]
    fn words_starting_with_region_are_not_markers() {
        assert_eq!(region_marker("// regional settings"), None);
        assert_eq!(region_marker("let region = 1;"), None);
    }

    #[test]
    fn a_region_folds_through_its_end_marker_over_nested_ones() {
        let text = Text::new("// #region outer\na\n// #region inner\nb\n// #endregion\n// #endregion\nafter");

        assert_eq!(region_fold_range(&text, 0), Some(FoldRange { header_line: 0, last_hidden_line: 5 }));
        assert_eq!(region_fold_range(&text, 2), Some(FoldRange { header_line: 2, last_hidden_line: 4 }));
        assert_eq!(region_fold_range(&text, 1), None);
    }

    #[test]
    fn a_region_without_an_end_does_not_fold() {
        assert_eq!(region_fold_range(&Text::new("// #region\na\nb"), 0), None);
    }

    #[test]
    fn every_region_is_listed_for_fold_all() {
        let text = Text::new("#region a\n#region b\n#endregion\n#endregion\n");

        let headers: Vec<usize> = region_fold_ranges(&text).iter().map(|range| range.header_line).collect();

        assert_eq!(headers, vec![0, 1]);
    }
}
