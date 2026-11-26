//! Indentation: tab stops, visual columns, detecting how a file is indented.

use crate::text::Text;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Indentation {
    /// Width of a tab character, in columns.
    pub tab_size: usize,
    /// Whether the indent unit is spaces (`tab_size` of them) or one tab.
    pub insert_spaces: bool,
}

impl Default for Indentation {
    fn default() -> Self {
        Self { tab_size: 4, insert_spaces: true }
    }
}

impl Indentation {
    /// What one level of indentation inserts.
    pub fn unit(&self) -> String {
        if self.insert_spaces { " ".repeat(self.tab_size) } else { "\t".to_string() }
    }

    /// Whitespace that fills `visual_columns` columns, with tabs where the unit is a tab.
    pub fn whitespace_for(&self, visual_columns: usize) -> String {
        if self.insert_spaces {
            return " ".repeat(visual_columns);
        }
        "\t".repeat(visual_columns / self.tab_size) + &" ".repeat(visual_columns % self.tab_size)
    }

    /// Visual column (tabs expanded to the next tab stop) of char `column` in `line`.
    pub fn visual_column(&self, line: &str, column: usize) -> usize {
        line.chars().take(column).fold(0, |visual_column, character| self.advance(visual_column, character))
    }

    /// Char column whose visual column is nearest to `visual_column` without passing it.
    pub fn column_at_visual(&self, line: &str, visual_column: usize) -> usize {
        let mut current_visual_column = 0;
        for (column, character) in line.chars().enumerate() {
            let next_visual_column = self.advance(current_visual_column, character);
            if next_visual_column > visual_column {
                let is_nearer_after = next_visual_column - visual_column < visual_column - current_visual_column;
                return if is_nearer_after { column + 1 } else { column };
            }
            current_visual_column = next_visual_column;
        }
        line.chars().count()
    }

    fn advance(&self, visual_column: usize, character: char) -> usize {
        if character == '\t' { (visual_column / self.tab_size + 1) * self.tab_size } else { visual_column + 1 }
    }

    /// Visual width of a line's leading whitespace.
    pub fn indent_width(&self, line: &str) -> usize {
        self.visual_column(line, leading_whitespace_len(line))
    }

    /// The visual column of the tab stop before `visual_column`.
    pub fn previous_tab_stop(&self, visual_column: usize) -> usize {
        if visual_column == 0 { 0 } else { (visual_column - 1) / self.tab_size * self.tab_size }
    }

    /// Guesses the indentation of a text: tabs when more lines start with a tab than with
    /// spaces, else the most common indent step between consecutive lines.
    /// Returns `fallback` when no line is indented.
    pub fn detect(text: &Text, fallback: Indentation) -> Indentation {
        const MAX_SAMPLED_LINES: usize = 10_000;

        let mut lines_with_tabs = 0;
        let mut lines_with_spaces = 0;
        let mut step_counts = [0usize; 9];
        let mut previous_space_indent = 0;
        for line in text.lines().take(MAX_SAMPLED_LINES) {
            if line.trim().is_empty() {
                continue;
            }
            if line.starts_with('\t') {
                lines_with_tabs += 1;
                continue;
            }
            let space_indent = line.chars().take_while(|&character| character == ' ').count();
            if space_indent > 0 {
                lines_with_spaces += 1;
            }
            let step = space_indent.abs_diff(previous_space_indent);
            if (2..=8).contains(&step) {
                step_counts[step] += 1;
            }
            previous_space_indent = space_indent;
        }

        if lines_with_tabs == 0 && lines_with_spaces == 0 {
            return fallback;
        }
        if lines_with_tabs > lines_with_spaces {
            return Indentation { tab_size: fallback.tab_size, insert_spaces: false };
        }
        let most_common_step = (2..=8).max_by_key(|&step| (step_counts[step], step == fallback.tab_size));
        Indentation { tab_size: most_common_step.unwrap_or(fallback.tab_size), insert_spaces: true }
    }
}

/// Number of whitespace chars at the start of a line.
pub fn leading_whitespace_len(line: &str) -> usize {
    line.chars().take_while(|character| *character == ' ' || *character == '\t').count()
}

pub fn leading_whitespace(line: &str) -> &str {
    let byte_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    &line[..byte_len]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_columns_expand_tabs_to_tab_stops() {
        let indentation = Indentation { tab_size: 4, insert_spaces: false };

        assert_eq!(indentation.visual_column("a\tb", 2), 4);
        assert_eq!(indentation.visual_column("\t\tb", 2), 8);
        assert_eq!(indentation.column_at_visual("a\tb", 4), 2);
        assert_eq!(indentation.column_at_visual("a\tb", 2), 1);
        assert_eq!(indentation.column_at_visual("a\tb", 3), 2);
        assert_eq!(indentation.column_at_visual("ab", 9), 2);
    }

    #[test]
    fn detects_two_space_indentation() {
        let text = Text::new("fn a() {\n  if b {\n    c();\n  }\n}\n");

        let detected = Indentation::detect(&text, Indentation::default());

        assert_eq!(detected, Indentation { tab_size: 2, insert_spaces: true });
    }

    #[test]
    fn detects_tabs() {
        let text = Text::new("a {\n\tb\n\tc\n}");

        assert!(!Indentation::detect(&text, Indentation::default()).insert_spaces);
    }

    #[test]
    fn falls_back_when_nothing_is_indented() {
        let fallback = Indentation { tab_size: 3, insert_spaces: false };

        assert_eq!(Indentation::detect(&Text::new("a\nb"), fallback), fallback);
    }

    #[test]
    fn previous_tab_stop() {
        let indentation = Indentation::default();

        assert_eq!(indentation.previous_tab_stop(6), 4);
        assert_eq!(indentation.previous_tab_stop(4), 0);
        assert_eq!(indentation.previous_tab_stop(0), 0);
    }
}
