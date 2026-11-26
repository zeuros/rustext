//! Word wrap: where a line breaks into continuation rows (DECISIONS D20).

/// Columns a char takes on screen: two for East Asian wide chars and most emoji, one otherwise
/// (tabs are handled by the caller).
pub fn char_columns(character: char) -> usize {
    let code = character as u32;
    let is_wide = matches!(code,
        0x1100..=0x115F | 0x2E80..=0x303E | 0x3041..=0x33FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F | 0x1F900..=0x1F9FF | 0x20000..=0x3FFFD);
    if is_wide { 2 } else { 1 }
}

/// Chars a line may break after (Monaco's `wordWrapBreakAfterCharacters`, plus whitespace).
fn breaks_after(character: char) -> bool {
    character.is_whitespace() || " \t})]?|/&.,;¢°′″‰℃、。｡､￠，．：；？！％・･ゝゞヽヾーァィゥェォッャュョヮヵヶぁぃぅぇぉっゃゅょゎゕゖㇰㇱㇲㇳㇴㇵㇶㇷㇸㇹㇺㇻㇼㇽㇾㇿ々〻ｧｨｩｪｫｬｭｮｯｰ”〉》」』】〕）］｝｣".contains(character)
}

/// The char columns where the continuation rows of `content` start when rows are
/// `width_columns` wide (empty when the line fits). Breaks after the last whitespace or
/// separator that fits, else inside the word.
pub fn wrap_points(content: &str, width_columns: usize, tab_size: usize) -> Vec<usize> {
    let width_columns = width_columns.max(1);
    let tab_size = tab_size.max(1);
    let mut points = Vec::new();
    let mut row_start_visual = 0;
    let mut visual = 0;
    let mut last_break_column: Option<usize> = None;
    let mut last_break_visual = 0;
    for (column, character) in content.chars().enumerate() {
        let width = if character == '\t' { tab_size - visual % tab_size } else { char_columns(character) };
        if visual + width - row_start_visual > width_columns && column > points.last().copied().unwrap_or(0) {
            let (break_column, break_visual) = match last_break_column {
                Some(break_column) if break_column > points.last().copied().unwrap_or(0) => {
                    (break_column, last_break_visual)
                }
                _ => (column, visual),
            };
            points.push(break_column);
            row_start_visual = break_visual;
            last_break_column = None;
        }
        visual += width;
        if breaks_after(character) {
            last_break_column = Some(column + 1);
            last_break_visual = visual;
        }
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_lines_do_not_wrap() {
        assert!(wrap_points("hello world", 20, 4).is_empty());
    }

    #[test]
    fn breaks_after_the_last_space_that_fits() {
        assert_eq!(wrap_points("hello brave new world", 12, 4), vec![12]);
        assert_eq!(wrap_points("aaaa bbbb cccc dddd", 10, 4), vec![10]);
    }

    #[test]
    fn breaks_inside_a_word_longer_than_a_row() {
        assert_eq!(wrap_points("abcdefghij", 4, 4), vec![4, 8]);
    }

    #[test]
    fn wide_chars_take_two_columns() {
        assert_eq!(wrap_points("漢字漢字", 4, 4), vec![2]);
    }

    #[test]
    fn tabs_count_to_their_tab_stop() {
        // The tab fills columns 0..4, `a` fits, `b` overflows: break after the whitespace.
        assert_eq!(wrap_points("\tab", 5, 4), vec![1]);
        assert_eq!(wrap_points("\tabc", 6, 4), vec![1]);
        assert_eq!(wrap_points("x\tyz", 5, 4), vec![2]);
    }

    #[test]
    fn breaks_after_separators() {
        assert_eq!(wrap_points("call(alpha,beta)", 12, 4), vec![11]);
    }
}
