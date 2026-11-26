//! Snippets (TextMate / VS Code syntax): `$1`, `${1:default}`, `$0` for the final cursor,
//! `\$` for a literal dollar. Parsed into plain text and tab stops in char offsets.

/// A tab stop: placeholder `index` covers `start..end` of the snippet's text. Several stops
/// with one index are edited together (mirrors).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TabStop {
    pub index: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snippet {
    pub text: String,
    /// Sorted by position.
    pub tab_stops: Vec<TabStop>,
}

impl Snippet {
    /// The tab stop indexes in visiting order: 1, 2, … then 0 (the final cursor) last.
    pub fn visiting_order(&self) -> Vec<usize> {
        let mut indexes: Vec<usize> = self.tab_stops.iter().map(|stop| stop.index).filter(|&index| index > 0).collect();
        indexes.sort_unstable();
        indexes.dedup();
        if self.tab_stops.iter().any(|stop| stop.index == 0) {
            indexes.push(0);
        }
        indexes
    }
}

/// Parses a snippet. Malformed placeholders are kept as text.
pub fn parse_snippet(input: &str) -> Snippet {
    let chars: Vec<char> = input.chars().collect();
    let mut snippet = Snippet::default();
    let mut text_len = 0;
    parse_into(&chars, &mut 0, &mut snippet, &mut text_len, false);
    snippet.tab_stops.sort_by_key(|stop| (stop.start, stop.end));
    snippet
}

/// Parses until the end, or a `}` closing a placeholder when `inside_placeholder`.
fn parse_into(
    chars: &[char],
    position: &mut usize,
    snippet: &mut Snippet,
    text_len: &mut usize,
    inside_placeholder: bool,
) {
    let push = |snippet: &mut Snippet, text_len: &mut usize, character: char| {
        snippet.text.push(character);
        *text_len += 1;
    };
    while *position < chars.len() {
        let character = chars[*position];
        match character {
            '\\' if chars.get(*position + 1).is_some_and(|next| matches!(next, '$' | '}' | '\\')) => {
                push(snippet, text_len, chars[*position + 1]);
                *position += 2;
            }
            '}' if inside_placeholder => {
                *position += 1;
                return;
            }
            '$' => {
                let digits: String =
                    chars[*position + 1..].iter().take_while(|character| character.is_ascii_digit()).collect();
                if !digits.is_empty() {
                    let index = digits.parse().unwrap_or(0);
                    snippet.tab_stops.push(TabStop { index, start: *text_len, end: *text_len });
                    *position += 1 + digits.len();
                    continue;
                }
                let braced_digits: String = chars
                    .get(*position + 2..)
                    .unwrap_or_default()
                    .iter()
                    .take_while(|character| character.is_ascii_digit())
                    .collect();
                let after_digits = *position + 2 + braced_digits.len();
                let is_placeholder = chars.get(*position + 1) == Some(&'{')
                    && !braced_digits.is_empty()
                    && matches!(chars.get(after_digits), Some(':' | '}'));
                if !is_placeholder {
                    push(snippet, text_len, character);
                    *position += 1;
                    continue;
                }
                let index = braced_digits.parse().unwrap_or(0);
                let start = *text_len;
                *position = after_digits + 1;
                if chars[after_digits] == ':' {
                    parse_into(chars, position, snippet, text_len, true);
                }
                snippet.tab_stops.push(TabStop { index, start, end: *text_len });
            }
            _ => {
                push(snippet, text_len, character);
                *position += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stops(snippet: &Snippet) -> Vec<(usize, usize, usize)> {
        snippet.tab_stops.iter().map(|stop| (stop.index, stop.start, stop.end)).collect()
    }

    #[test]
    fn parses_placeholders_and_plain_stops() {
        let snippet = parse_snippet("for ${1:item} in ${2:items} {\n\t$0\n}");

        assert_eq!(snippet.text, "for item in items {\n\t\n}");
        assert_eq!(stops(&snippet), vec![(1, 4, 8), (2, 12, 17), (0, 21, 21)]);
        assert_eq!(snippet.visiting_order(), vec![1, 2, 0]);
    }

    #[test]
    fn mirrors_share_an_index() {
        let snippet = parse_snippet("${1:x} = $1");

        assert_eq!(snippet.text, "x = ");
        assert_eq!(stops(&snippet), vec![(1, 0, 1), (1, 4, 4)]);
    }

    #[test]
    fn nested_placeholders_and_escapes() {
        let snippet = parse_snippet("${1:a ${2:b}} \\$3 $ cost");

        assert_eq!(snippet.text, "a b $3 $ cost");
        assert_eq!(stops(&snippet), vec![(1, 0, 3), (2, 2, 3)]);
    }
}
