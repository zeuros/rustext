//! Telling code from strings and comments, line by line, from the language configuration alone
//! (comment markers and quotes): enough to ignore the brackets inside them without a parser.

use crate::config::LanguageConfiguration;

/// What a line starts (or ends) inside.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LexicalContext {
    #[default]
    Code,
    BlockComment,
    /// A string opened by this quote and not closed yet.
    String(char),
}

/// The brackets of a line that are code, and the context the next line starts in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineScan {
    /// `(column, bracket)`, in column order.
    pub brackets: Vec<(usize, char)>,
    pub context_at_end: LexicalContext,
}

/// Scans `line_content` (without its line ending) starting in `context`.
pub fn scan_line(line_content: &str, context: LexicalContext, language: &LanguageConfiguration) -> LineScan {
    scan_line_chars(line_content.chars(), context, language)
}

/// `scan_line` over the chars of a line, read straight from the text.
pub fn scan_line_chars(
    line_chars: impl Iterator<Item = char>,
    context: LexicalContext,
    language: &LanguageConfiguration,
) -> LineScan {
    let chars: Vec<char> = line_chars.collect();
    let starts_with = |column: usize, marker: &str| {
        let mut marker_chars = marker.chars();
        !marker.is_empty()
            && marker_chars.by_ref().enumerate().all(|(index, character)| chars.get(column + index) == Some(&character))
    };
    let mut brackets = Vec::new();
    let mut context = context;
    let mut column = 0;
    while column < chars.len() {
        let character = chars[column];
        match context {
            LexicalContext::BlockComment => {
                let close = language.block_comment.as_ref().map(|(_, close)| close.as_str()).unwrap_or_default();
                if starts_with(column, close) {
                    context = LexicalContext::Code;
                    column += close.chars().count();
                    continue;
                }
            }
            LexicalContext::String(quote) => {
                if character == '\\' {
                    column += 2;
                    continue;
                }
                if character == quote {
                    context = LexicalContext::Code;
                }
            }
            LexicalContext::Code => {
                if language.line_comment.as_deref().is_some_and(|marker| starts_with(column, marker)) {
                    break;
                }
                if let Some((open, _)) = &language.block_comment
                    && starts_with(column, open)
                {
                    context = LexicalContext::BlockComment;
                    column += open.chars().count();
                    continue;
                }
                if language.string_quotes.contains(&character) {
                    context = LexicalContext::String(character);
                } else if language.char_quote == Some(character) {
                    column += char_literal_len(&chars[column..], character).unwrap_or(1);
                    continue;
                } else if language.is_bracket(character) {
                    brackets.push((column, character));
                }
            }
        }
        column += 1;
    }
    LineScan { brackets, context_at_end: context }
}

/// Length of the char literal at the start of `chars` (`'x'`, `'\n'`, `'\u{1F600}'`), `None`
/// when the quote doesn't start one (a Rust lifetime, a prime).
fn char_literal_len(chars: &[char], quote: char) -> Option<usize> {
    const LONGEST_ESCAPE: usize = 12;
    match chars.get(1)? {
        '\\' => {
            chars.iter().skip(3).take(LONGEST_ESCAPE).position(|&character| character == quote).map(|index| index + 4)
        }
        _ => (chars.get(2) == Some(&quote)).then_some(3),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rust() -> LanguageConfiguration {
        LanguageConfiguration::c_like()
    }

    fn bracket_columns(scan: &LineScan) -> Vec<usize> {
        scan.brackets.iter().map(|&(column, _)| column).collect()
    }

    #[test]
    fn brackets_in_strings_and_line_comments_are_not_code() {
        let scan = scan_line(r#"f("(", x) // (comment)"#, LexicalContext::Code, &rust());

        assert_eq!(bracket_columns(&scan), vec![1, 8]);
        assert_eq!(scan.context_at_end, LexicalContext::Code);
    }

    #[test]
    fn escaped_quotes_do_not_close_a_string() {
        let scan = scan_line(r#""a\"(" )"#, LexicalContext::Code, &rust());

        assert_eq!(bracket_columns(&scan), vec![7]);
    }

    #[test]
    fn block_comments_and_strings_carry_to_the_next_line() {
        let language = rust();

        let comment = scan_line("a /* (", LexicalContext::Code, &language);
        assert_eq!(comment.context_at_end, LexicalContext::BlockComment);
        let after = scan_line(") */ (", comment.context_at_end, &language);
        assert_eq!(bracket_columns(&after), vec![5]);

        let string = scan_line(r#"let s = "("#, LexicalContext::Code, &language);
        assert_eq!(string.context_at_end, LexicalContext::String('"'));
    }

    #[test]
    fn char_literals_hide_their_bracket_but_lifetimes_are_code() {
        let scan = scan_line(r"fn f<'a>(c: '(', e: '\u{28}') -> &'a str {", LexicalContext::Code, &rust());

        assert_eq!(bracket_columns(&scan), vec![8, 28, 41]);
    }

    #[test]
    fn plain_text_has_no_strings() {
        let scan = scan_line("don't (really)", LexicalContext::Code, &LanguageConfiguration::plain_text());

        assert_eq!(bracket_columns(&scan), vec![6, 13]);
    }
}
