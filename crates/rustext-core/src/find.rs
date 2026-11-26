//! Find and replace over a text: a typed query compiled to one regex, matches in char offsets,
//! replacements with `$1` groups and case preserving (Monaco's `findMatches` and the find
//! widget's replace).

use regex::{Regex, RegexBuilder};
use regex_cursor::engines::meta::Regex as RopeRegex;
use regex_cursor::regex_automata::util::syntax::Config as SyntaxConfig;
use regex_cursor::{Input, RopeyCursor};

use crate::change::{Change, ChangeSet};
use crate::edit::Transaction;
use crate::history::EditKind;
use crate::selection::{Selection, Selections};
use crate::text::Text;

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct SearchQuery {
    pub pattern: String,
    pub is_regex: bool,
    pub match_case: bool,
    pub whole_word: bool,
    /// Replacing `foo` by `bar` writes `Bar` over `Foo` and `BAR` over `FOO`.
    pub preserve_case: bool,
}

/// A match, in char offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Match {
    pub start: usize,
    pub end: usize,
}

/// `text` as a regex matching it literally.
pub fn escape(text: &str) -> String {
    regex::escape(text)
}

impl SearchQuery {
    pub fn new(pattern: impl Into<String>) -> Self {
        Self { pattern: pattern.into(), ..Self::default() }
    }

    /// The pattern searched: the query escaped unless it is a regex, inside word boundaries
    /// for whole words.
    fn regex_pattern(&self) -> String {
        let pattern = if self.is_regex { self.pattern.clone() } else { regex::escape(&self.pattern) };
        if self.whole_word { format!(r"\b(?:{pattern})\b") } else { pattern }
    }

    /// The regex expanding `$1` groups in one match; an error message for an invalid regex.
    pub fn regex(&self) -> Result<Regex, String> {
        RegexBuilder::new(&self.regex_pattern())
            .case_insensitive(!self.match_case)
            .multi_line(true)
            .build()
            .map_err(|error| error.to_string())
    }

    /// The regex searching the rope chunk by chunk (regex-cursor, Helix's), with the same
    /// syntax as `regex`.
    fn rope_regex(&self) -> Result<RopeRegex, String> {
        let syntax = SyntaxConfig::new().case_insensitive(!self.match_case).multi_line(true);
        RopeRegex::builder().syntax(syntax).build(&self.regex_pattern()).map_err(|error| error.to_string())
    }
}

/// Every non-empty match of `query` in `text`, at most `limit`, optionally only inside the
/// char offsets `within` (find in selection). Searches the rope in place: nothing is copied.
pub fn find_matches(
    text: &Text,
    query: &SearchQuery,
    within: Option<(usize, usize)>,
    limit: usize,
) -> Result<Vec<Match>, String> {
    if query.pattern.is_empty() {
        return Ok(Vec::new());
    }
    let regex = query.rope_regex()?;
    let (start, end) = within.unwrap_or((0, text.len_chars()));
    let searched = text.rope().slice(start..end);

    let matches = regex
        .find_iter(Input::new(RopeyCursor::new(searched)))
        .filter(|found| !found.is_empty())
        .take(limit)
        .map(|found| Match {
            start: start + searched.byte_to_char(found.start()),
            end: start + searched.byte_to_char(found.end()),
        })
        .collect();
    Ok(matches)
}

/// Index of the first match at or after `offset`, wrapping to the first match.
pub fn match_at_or_after(matches: &[Match], offset: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(matches.iter().position(|found| found.start >= offset).unwrap_or(0))
}

/// Index of the last match before `offset`, wrapping to the last match.
pub fn match_before(matches: &[Match], offset: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(matches.iter().rposition(|found| found.start < offset).unwrap_or(matches.len() - 1))
}

/// What replaces one match: `$1`-style groups expanded for a regex query, case preserved
/// when asked.
pub fn replacement_for(text: &Text, found: Match, query: &SearchQuery, replacement: &str) -> String {
    let matched = text.slice(found.start, found.end);
    let expanded = match query.regex() {
        Ok(regex) if query.is_regex => regex.replace(&matched, replacement).into_owned(),
        _ => replacement.to_string(),
    };
    if query.preserve_case { with_case_of(&matched, &expanded) } else { expanded }
}

/// `replacement` written in the case of `original`: all upper, all lower, or capitalized.
fn with_case_of(original: &str, replacement: &str) -> String {
    let letters: Vec<char> = original.chars().filter(|character| character.is_alphabetic()).collect();
    if letters.len() > 1 && letters.iter().all(|character| character.is_uppercase()) {
        return replacement.to_uppercase();
    }
    if !letters.is_empty() && letters.iter().all(|character| character.is_lowercase()) {
        return replacement.to_lowercase();
    }
    if letters.first().is_some_and(|character| character.is_uppercase()) {
        let mut chars = replacement.chars();
        return chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default();
    }
    replacement.to_string()
}

/// Replaces every match; the cursor ends after the last replacement.
pub fn replace_all(text: &Text, matches: &[Match], query: &SearchQuery, replacement: &str) -> Option<Transaction> {
    if matches.is_empty() {
        return None;
    }
    let changes = ChangeSet::new(
        matches
            .iter()
            .map(|&found| Change::new(found.start, found.end, replacement_for(text, found, query, replacement))),
    );
    let last = matches[matches.len() - 1];
    let cursor = changes.map_offset(last.end, crate::change::Bias::After);
    Some(Transaction {
        changes,
        selections: Selections::cursor(cursor),
        kind: EditKind::Other,
        auto_closed_offsets: Vec::new(),
    })
}

/// Replaces one match and selects the replacement.
pub fn replace_one(text: &Text, found: Match, query: &SearchQuery, replacement: &str) -> Transaction {
    let replaced = replacement_for(text, found, query, replacement);
    let replaced_len = replaced.chars().count();
    let changes = ChangeSet::single(Change::new(found.start, found.end, replaced));
    Transaction {
        changes,
        selections: Selections::single(Selection::new(found.start, found.start + replaced_len)),
        kind: EditKind::Other,
        auto_closed_offsets: Vec::new(),
    }
}

/// Every match as a selection (Alt+Enter in the find widget).
pub fn select_matches(matches: &[Match], current_index: Option<usize>) -> Option<Selections> {
    if matches.is_empty() {
        return None;
    }
    let ranges = matches.iter().map(|found| Selection::new(found.start, found.end)).collect();
    Some(Selections::new(ranges, current_index.unwrap_or(0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offsets(matches: &[Match]) -> Vec<(usize, usize)> {
        matches.iter().map(|found| (found.start, found.end)).collect()
    }

    #[test]
    fn plain_search_ignores_case_by_default_and_escapes_regex_chars() {
        let text = Text::new("Foo foo f.o");

        assert_eq!(offsets(&find_matches(&text, &SearchQuery::new("foo"), None, 100).unwrap()), vec![(0, 3), (4, 7)]);
        assert_eq!(offsets(&find_matches(&text, &SearchQuery::new("f.o"), None, 100).unwrap()), vec![(8, 11)]);
    }

    #[test]
    fn match_case_and_whole_word() {
        let text = Text::new("cat Cat concat");
        let query = SearchQuery { match_case: true, whole_word: true, ..SearchQuery::new("cat") };

        assert_eq!(offsets(&find_matches(&text, &query, None, 100).unwrap()), vec![(0, 3)]);
    }

    #[test]
    fn regex_offsets_are_in_chars() {
        let text = Text::new("é1 é22");
        let query = SearchQuery { is_regex: true, ..SearchQuery::new(r"\d+") };

        assert_eq!(offsets(&find_matches(&text, &query, None, 100).unwrap()), vec![(1, 2), (4, 6)]);
    }

    #[test]
    fn matches_across_rope_chunks_are_found_at_their_char_offsets() {
        let filler = "é".repeat(5_000);
        let text = Text::new(&format!("{filler}needle{filler}needle"));

        let matches = find_matches(&text, &SearchQuery::new("needle"), None, 10).unwrap();

        assert_eq!(offsets(&matches), vec![(5_000, 5_006), (10_006, 10_012)]);
    }

    #[test]
    fn invalid_regex_is_an_error() {
        let query = SearchQuery { is_regex: true, ..SearchQuery::new("(") };

        assert!(find_matches(&Text::new("x"), &query, None, 10).is_err());
    }

    #[test]
    fn search_within_a_range() {
        let text = Text::new("ab ab ab");

        let matches = find_matches(&text, &SearchQuery::new("ab"), Some((2, 6)), 100).unwrap();

        assert_eq!(offsets(&matches), vec![(3, 5)]);
    }

    #[test]
    fn caret_and_dollar_match_line_boundaries() {
        let text = Text::new("a\nab\n");
        let query = SearchQuery { is_regex: true, ..SearchQuery::new("^a") };

        assert_eq!(find_matches(&text, &query, None, 100).unwrap().len(), 2);
    }

    #[test]
    fn replaces_with_groups() {
        let text = Text::new("let x = f(a, b);");
        let query = SearchQuery { is_regex: true, ..SearchQuery::new(r"f\((\w+), (\w+)\)") };
        let matches = find_matches(&text, &query, None, 10).unwrap();

        let transaction = replace_all(&text, &matches, &query, "f($2, $1)").unwrap();

        assert_eq!(transaction.changes.apply(&text).to_string(), "let x = f(b, a);");
    }

    #[test]
    fn preserve_case() {
        let text = Text::new("foo Foo FOO");
        let query = SearchQuery { preserve_case: true, ..SearchQuery::new("foo") };
        let matches = find_matches(&text, &query, None, 10).unwrap();

        let transaction = replace_all(&text, &matches, &query, "bar").unwrap();

        assert_eq!(transaction.changes.apply(&text).to_string(), "bar Bar BAR");
    }

    #[test]
    fn next_and_previous_wrap() {
        let matches = [Match { start: 2, end: 3 }, Match { start: 8, end: 9 }];

        assert_eq!(match_at_or_after(&matches, 3), Some(1));
        assert_eq!(match_at_or_after(&matches, 9), Some(0));
        assert_eq!(match_before(&matches, 8), Some(0));
        assert_eq!(match_before(&matches, 2), Some(1));
    }
}
