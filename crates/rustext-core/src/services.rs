//! Language services as a trait the application implements (DECISIONS D9): completion, hover,
//! go to definition. Each method returns plain data, or `Reply::Pending` when the answer is not
//! ready yet (an LSP request in flight): the editor asks again on a later frame. Every method
//! has a default answering "nothing", so a service implements only what it knows.

use crate::position::{Position, Range};
use crate::text::Text;
use crate::words::WordDefinition;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply<T> {
    Ready(T),
    /// Not known yet: ask again later.
    Pending,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CompletionKind {
    #[default]
    Text,
    Keyword,
    Function,
    Method,
    Variable,
    Field,
    Type,
    Module,
    Constant,
    Snippet,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompletionItem {
    /// What the list shows, and what the typed prefix is matched against.
    pub label: String,
    pub kind: CompletionKind,
    /// A short description shown next to the label (a type, a signature).
    pub detail: Option<String>,
    /// What accepting inserts in place of the prefix; the label when `None`.
    pub insert_text: Option<String>,
    /// `insert_text` is a snippet (`${1:arg}`…): its tab stops are visited with Tab.
    pub is_snippet: bool,
}

impl CompletionItem {
    pub fn new(label: impl Into<String>, kind: CompletionKind) -> Self {
        Self { label: label.into(), kind, ..Self::default() }
    }

    pub fn with_detail(self, detail: impl Into<String>) -> Self {
        Self { detail: Some(detail.into()), ..self }
    }

    /// A snippet item: `snippet` (`fn ${1:name}() {$0}`) is inserted with its tab stops.
    pub fn snippet(label: impl Into<String>, snippet: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            kind: CompletionKind::Snippet,
            insert_text: Some(snippet.into()),
            is_snippet: true,
            ..Self::default()
        }
    }

    pub fn text_to_insert(&self) -> &str {
        self.insert_text.as_deref().unwrap_or(&self.label)
    }
}

/// How bad a diagnostic is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

/// A problem in the text, from a compiler or linter (Monaco's marker).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: Severity,
    pub message: String,
}

/// A text replacing a range of the document, from a service (rename, formatting).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}

/// The signature of the call around the cursor, and which parameter it is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureHelp {
    /// `fn push(&mut self, value: T)`.
    pub label: String,
    /// Char columns in `label` of the parameter the cursor is in.
    pub active_parameter: Option<(usize, usize)>,
    pub documentation: Option<String>,
}

/// A label shown inside the text, not part of it: a parameter name, an inferred type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlayHint {
    pub position: Position,
    /// Shown as is: services put the padding spaces they want in it.
    pub label: String,
}

/// The edits of `edits` as one transaction (they must not overlap), the cursor after the
/// first one.
pub fn transaction_of_edits(text: &Text, edits: &[TextEdit]) -> Option<crate::edit::Transaction> {
    use crate::change::{Bias, Change, ChangeSet};
    let first = edits.first()?;
    let changes = ChangeSet::new(edits.iter().map(|edit| {
        let (start, end) = text.range_offsets(edit.range);
        Change::new(start, end, edit.new_text.clone())
    }));
    let cursor = changes.map_offset(text.offset(first.range.start), Bias::Before) + first.new_text.chars().count();
    Some(crate::edit::Transaction {
        changes,
        selections: crate::selection::Selections::cursor(cursor),
        kind: crate::history::EditKind::Other,
        auto_closed_offsets: Vec::new(),
    })
}

/// What the editor asks about a text. `text_revision` identifies the text's version, so a
/// service can drop answers computed for an older one.
pub trait LanguageServices {
    /// The text changed: `changes[i]` applied to `texts_before[i]`, leaving revision
    /// `text_revision_after`. Lets a service follow edits instead of reading the whole text.
    fn text_changed(
        &mut self,
        _texts_before: &[Text],
        _changes: &[crate::change::ChangeSet],
        _text_revision_after: u64,
    ) {
    }

    /// Completions at `position`; the editor filters and sorts them by the typed prefix.
    fn completions(&mut self, _text: &Text, _text_revision: u64, _position: Position) -> Reply<Vec<CompletionItem>> {
        Reply::Ready(Vec::new())
    }

    /// Chars that open the completion list right after being typed (`.` for members).
    fn completion_trigger_chars(&self) -> &[char] {
        &[]
    }

    /// Text to suggest after `position` as ghost text (Monaco's inline completions: what an AI
    /// assistant proposes); Tab accepts it.
    fn inline_completion(&mut self, _text: &Text, _text_revision: u64, _position: Position) -> Reply<Option<String>> {
        Reply::Ready(None)
    }

    /// What to show when hovering `position` (documentation, a type).
    fn hover(&mut self, _text: &Text, _text_revision: u64, _position: Position) -> Reply<Option<String>> {
        Reply::Ready(None)
    }

    /// The diagnostics the service published since the last call (they replace the previous
    /// ones), `None` when nothing new arrived. Services push them when they like (LSP's
    /// `publishDiagnostics`), so the editor asks every frame.
    fn take_diagnostics(&mut self, _text: &Text) -> Option<Vec<Diagnostic>> {
        None
    }

    /// The edits renaming the symbol at `position` to `new_name` in this text (F2).
    fn rename(
        &mut self,
        _text: &Text,
        _text_revision: u64,
        _position: Position,
        _new_name: &str,
    ) -> Reply<Vec<TextEdit>> {
        Reply::Ready(Vec::new())
    }

    /// The signature of the call around `position` (shown after `(` and `,`).
    fn signature_help(
        &mut self,
        _text: &Text,
        _text_revision: u64,
        _position: Position,
    ) -> Reply<Option<SignatureHelp>> {
        Reply::Ready(None)
    }

    /// The inlay hints of `lines` (the editor asks for a window around the visible lines).
    fn inlay_hints(
        &mut self,
        _text: &Text,
        _text_revision: u64,
        _lines: std::ops::Range<usize>,
    ) -> Reply<Vec<InlayHint>> {
        Reply::Ready(Vec::new())
    }

    /// Where the symbol at `position` is defined, in the same text (F12, Ctrl+click).
    fn definition(&mut self, _text: &Text, _text_revision: u64, _position: Position) -> Reply<Option<Position>> {
        Reply::Ready(None)
    }
}

/// Monaco's `wordBasedSuggestions`: the words of the text itself, near the cursor first.
#[derive(Clone, Debug, Default)]
pub struct WordCompletions {
    pub words: WordDefinition,
}

impl LanguageServices for WordCompletions {
    fn completions(&mut self, text: &Text, _text_revision: u64, position: Position) -> Reply<Vec<CompletionItem>> {
        const MAX_SCANNED_LINES: usize = 5_000;
        let mut seen = std::collections::HashSet::new();
        let mut items = Vec::new();
        let first_line = position.line.saturating_sub(MAX_SCANNED_LINES / 2);
        let last_line = (position.line + MAX_SCANNED_LINES / 2).min(text.last_line());
        // Nearest lines first, so closer words come first among equal scores.
        let mut lines: Vec<usize> = (first_line..=last_line).collect();
        lines.sort_by_key(|line| line.abs_diff(position.line));
        for line in lines {
            let content: Vec<char> = text.line_content(line).chars().collect();
            let mut column = 0;
            while column < content.len() {
                let Some((start, end)) = self.words.word_at(&content, column).filter(|(start, _)| *start == column)
                else {
                    column += 1;
                    continue;
                };
                let touches_cursor = line == position.line && (start..=end).contains(&position.column);
                let word: String = content[start..end].iter().collect();
                if !touches_cursor && word.chars().count() > 1 && seen.insert(word.clone()) {
                    items.push(CompletionItem::new(word, CompletionKind::Text));
                }
                column = end;
            }
        }
        Reply::Ready(items)
    }
}

/// How well `query` matches `candidate` as a subsequence (VS Code's fuzzy matching), higher is
/// better; `None` when it doesn't match. Case-insensitive; matches at the start, at word
/// starts (`_x`, `camelCase` humps) and consecutive chars score more.
pub fn fuzzy_score(query: &str, candidate: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let candidate_chars: Vec<char> = candidate.chars().collect();
    let mut score = 0i64;
    let mut candidate_index = 0;
    let mut previous_match: Option<usize> = None;
    for query_char in query.chars() {
        let query_lower = query_char.to_lowercase().next().unwrap_or(query_char);
        let found = (candidate_index..candidate_chars.len())
            .find(|&index| candidate_chars[index].to_lowercase().next() == Some(query_lower))?;
        let character = candidate_chars[found];
        let starts_word = found == 0
            || !candidate_chars[found - 1].is_alphanumeric()
            || (candidate_chars[found - 1].is_lowercase() && character.is_uppercase());
        score += 1;
        if found == 0 {
            score += 8;
        }
        if starts_word {
            score += 5;
        }
        if previous_match.is_some_and(|previous| previous + 1 == found) {
            score += 4;
        }
        if character == query_char {
            score += 1;
        }
        score -= (found - candidate_index) as i64 / 2;
        previous_match = Some(found);
        candidate_index = found + 1;
    }
    score -= (candidate_chars.len() as i64 - query.chars().count() as i64) / 4;
    Some(score)
}

/// The items matching `query`, best first (ties keep their order).
pub fn filter_completions(items: &[CompletionItem], query: &str) -> Vec<CompletionItem> {
    let mut scored: Vec<(i64, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| fuzzy_score(query, &item.label).map(|score| (score, index)))
        .collect();
    scored.sort_by_key(|&(score, index)| (std::cmp::Reverse(score), index));
    scored.into_iter().map(|(_, index)| items[index].clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_matches_subsequences_and_prefers_word_starts() {
        assert!(fuzzy_score("gtl", "getLine").is_some());
        assert!(fuzzy_score("xyz", "getLine").is_none());
        assert!(fuzzy_score("gl", "getLine") > fuzzy_score("gl", "angle"));
        assert!(fuzzy_score("len", "length") > fuzzy_score("len", "is_lenient"));
    }

    #[test]
    fn edits_become_one_transaction() {
        let text = Text::new("let a = a + 1;");
        let edits = [
            TextEdit { range: Range::new(Position::new(0, 4), Position::new(0, 5)), new_text: "total".into() },
            TextEdit { range: Range::new(Position::new(0, 8), Position::new(0, 9)), new_text: "total".into() },
        ];

        let transaction = transaction_of_edits(&text, &edits).unwrap();

        assert_eq!(transaction.changes.apply(&text).to_string(), "let total = total + 1;");
        assert_eq!(transaction.selections.primary().head, 9);
    }

    #[test]
    fn filtering_sorts_best_first() {
        let items: Vec<CompletionItem> = ["toString", "to_owned", "iter", "into_iter"]
            .iter()
            .map(|label| CompletionItem::new(*label, CompletionKind::Method))
            .collect();

        let labels: Vec<String> = filter_completions(&items, "it").into_iter().map(|item| item.label).collect();

        assert_eq!(labels, vec!["iter", "into_iter"]);
    }

    #[test]
    fn word_completions_come_from_the_text_near_the_cursor_first() {
        let text = Text::new("alpha beta\ngamma\nal");
        let mut words = WordCompletions::default();

        let Reply::Ready(items) = words.completions(&text, 0, Position::new(2, 2)) else { panic!("ready") };

        let labels: Vec<String> = items.into_iter().map(|item| item.label).collect();
        assert_eq!(labels, vec!["gamma", "alpha", "beta"]);
    }
}
