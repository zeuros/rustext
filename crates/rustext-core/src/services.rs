//! Language services as a trait the application implements (DECISIONS D9): completion, hover,
//! go to definition. Each method returns plain data, or `Reply::Pending` when the answer is not
//! ready yet (an LSP request in flight): the editor asks again on a later frame. Every method
//! has a default answering "nothing", so a service implements only what it knows.

use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::position::{Position, Range};
use crate::text::Text;
use crate::words::WordDefinition;

/// A range of this document (`document: None`) or of another one, named as the services name
/// it (an LSP client: by file URI). Columns are chars, in every document (D5).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Location {
    pub document: Option<String>,
    pub range: Range,
}

impl Location {
    pub fn here(range: Range) -> Self {
        Self { document: None, range }
    }

    pub fn in_document(document: impl Into<String>, range: Range) -> Self {
        Self { document: Some(document.into()), range }
    }
}

/// A change the services offer at a place (Ctrl+.): a quick fix, a refactoring.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeAction {
    pub title: String,
    /// Fixes a diagnostic.
    pub is_quick_fix: bool,
    /// The one to pick when unsure (VS Code's auto-fix).
    pub is_preferred: bool,
    /// Its edits to this document, applied as one undo step.
    pub edits: Vec<TextEdit>,
}

/// What a document symbol is, for its icon or label.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Method,
    Type,
    Module,
    Constant,
    Variable,
    Field,
    Other,
}

impl SymbolKind {
    pub fn label(self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Type => "type",
            SymbolKind::Module => "module",
            SymbolKind::Constant => "constant",
            SymbolKind::Variable => "variable",
            SymbolKind::Field => "field",
            SymbolKind::Other => "symbol",
        }
    }
}

/// A named definition of the document (Go to Symbol, Ctrl+Shift+O): `range` is its name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub range: Range,
    /// The symbol holding it (a method's type), when known.
    pub container: Option<String>,
}

/// A clickable label shown above a line (Monaco's code lens): the language services' ("3
/// implementations", "Run test") or the application's own ("Stage hunk", a blame summary).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CodeLens {
    pub line: usize,
    pub label: String,
    pub action: CodeLensAction,
}

impl CodeLens {
    /// An application's lens: a click on it is reported.
    pub fn new(line: usize, label: impl Into<String>) -> Self {
        Self { line, label: label.into(), action: CodeLensAction::Report }
    }
}

/// What a click on a code lens does.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CodeLensAction {
    /// Reported to the application (`EditorResponse::clicked_code_lens`); a services' lens
    /// with nothing to run does nothing.
    Report,
    /// Lists these places, as Find References does ("3 implementations").
    ShowLocations(Vec<Location>),
    /// A command of the language services for the application to run.
    Run(ServiceCommand),
}

/// A command the language services offer and only the application can run (LSP's commands:
/// `workspace/executeCommand`, or one the client implements such as rust-analyzer's
/// `rust-analyzer.runSingle`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ServiceCommand {
    /// What the services call it (`rust-analyzer.runSingle`).
    pub name: String,
    pub title: String,
    /// Its arguments, as the services encode them (LSP: a JSON array).
    pub arguments: String,
}

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
    use crate::change::Bias;
    let first = edits.first()?;
    let changes = changes_of_edits(text, edits);
    let cursor = changes.map_offset(text.offset(first.range.start), Bias::Before) + first.new_text.chars().count();
    Some(crate::edit::Transaction {
        changes,
        selections: crate::selection::Selections::cursor(cursor),
        kind: crate::history::EditKind::Other,
        auto_closed_offsets: Vec::new(),
    })
}

/// Edits as one change set on `text`.
fn changes_of_edits(text: &Text, edits: &[TextEdit]) -> crate::change::ChangeSet {
    crate::change::ChangeSet::new(edits.iter().map(|edit| {
        let (start, end) = text.range_offsets(edit.range);
        crate::change::Change::new(start, end, edit.new_text.clone())
    }))
}

/// Edits that rewrite parts of the text (formatting) as one transaction keeping `selections`
/// where they were (mapped through the edits); `None` without edits.
pub fn transaction_keeping_selections(
    text: &Text,
    edits: &[TextEdit],
    selections: &crate::selection::Selections,
) -> Option<crate::edit::Transaction> {
    if edits.is_empty() {
        return None;
    }
    let changes = changes_of_edits(text, edits);
    Some(crate::edit::Transaction {
        selections: selections.map(&changes),
        changes,
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

    /// The symbol at `position` and its other uses in the text, highlighted while the cursor
    /// is on it; `None` when the service doesn't know (the editor highlights the same word).
    fn document_highlights(
        &mut self,
        _text: &Text,
        _text_revision: u64,
        _position: Position,
    ) -> Reply<Option<Vec<crate::position::Range>>> {
        Reply::Ready(None)
    }

    /// The uses of the symbol at `position`, its declaration included (Shift+F12), in this
    /// document and others.
    fn references(&mut self, _text: &Text, _text_revision: u64, _position: Position) -> Reply<Vec<Location>> {
        Reply::Ready(Vec::new())
    }

    /// The actions offered for `range` (the selection, or the cursor), preferred ones first.
    fn code_actions(&mut self, _text: &Text, _text_revision: u64, _range: Range) -> Reply<Vec<CodeAction>> {
        Reply::Ready(Vec::new())
    }

    /// Edits formatting the whole document with this indentation (Shift+Alt+F).
    fn format_document(
        &mut self,
        _text: &Text,
        _text_revision: u64,
        _indentation: crate::indentation::Indentation,
    ) -> Reply<Vec<TextEdit>> {
        Reply::Ready(Vec::new())
    }

    /// Edits formatting the lines of `range` (Ctrl+K Ctrl+F).
    fn format_range(
        &mut self,
        _text: &Text,
        _text_revision: u64,
        _range: Range,
        _indentation: crate::indentation::Indentation,
    ) -> Reply<Vec<TextEdit>> {
        Reply::Ready(Vec::new())
    }

    /// Chars that format the code around the cursor right after being typed (Monaco's
    /// `formatOnType`; `\n` is Enter).
    fn format_on_type_trigger_chars(&self) -> &[char] {
        &[]
    }

    /// Edits formatting the code around `position`, right after `typed` was typed before it.
    #[expect(clippy::too_many_arguments, reason = "predates the complexity lints (D28)")]
    fn format_on_type(
        &mut self,
        _text: &Text,
        _text_revision: u64,
        _position: Position,
        _typed: char,
        _indentation: crate::indentation::Indentation,
    ) -> Reply<Vec<TextEdit>> {
        Reply::Ready(Vec::new())
    }

    /// The code lenses of the document, shown with the application's own.
    fn code_lenses(&mut self, _text: &Text, _text_revision: u64) -> Reply<Vec<CodeLens>> {
        Reply::Ready(Vec::new())
    }

    /// The definitions of the document (Go to Symbol), in text order. Without an answer, the
    /// styler's syntax tree gives them.
    fn document_symbols(&mut self, _text: &Text, _text_revision: u64) -> Reply<Vec<DocumentSymbol>> {
        Reply::Ready(Vec::new())
    }

    /// Where the symbol at `position` is defined (F12, Ctrl+click), in this document or another.
    fn definition(&mut self, _text: &Text, _text_revision: u64, _position: Position) -> Reply<Option<Location>> {
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

/// A fuzzy query (nucleo, Helix's matcher): matches candidates as subsequences, ignoring case,
/// scoring word starts, `camelCase` humps, consecutive chars and prefixes higher. Holds the
/// matcher's buffers, so one query scores many candidates without allocating.
pub struct FuzzyQuery {
    atom: Atom,
    matcher: Matcher,
    candidate_chars: Vec<char>,
}

impl FuzzyQuery {
    pub fn new(query: &str) -> Self {
        let mut config = Config::DEFAULT;
        config.prefer_prefix = true;
        Self {
            atom: Atom::new(query, CaseMatching::Ignore, Normalization::Smart, AtomKind::Fuzzy, false),
            matcher: Matcher::new(config),
            candidate_chars: Vec::new(),
        }
    }

    /// How well the query matches `candidate`, higher is better; `None` when it doesn't match.
    /// An empty query matches everything equally.
    pub fn score(&mut self, candidate: &str) -> Option<u16> {
        if self.atom.needle_text().is_empty() {
            return Some(0);
        }
        self.atom.score(Utf32Str::new(candidate, &mut self.candidate_chars), &mut self.matcher)
    }
}

/// The items matching `query`, best first; equal scores put the shorter label first, then keep
/// their order.
pub fn filter_completions(items: &[CompletionItem], query: &str) -> Vec<CompletionItem> {
    let mut fuzzy_query = FuzzyQuery::new(query);
    let mut scored: Vec<(u16, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| fuzzy_query.score(&item.label).map(|score| (score, index)))
        .collect();
    scored.sort_by_key(|&(score, index)| (std::cmp::Reverse(score), items[index].label.chars().count(), index));
    scored.into_iter().map(|(_, index)| items[index].clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_matches_subsequences_and_prefers_word_starts() {
        let score = |query: &str, candidate: &str| FuzzyQuery::new(query).score(candidate);

        assert!(score("gtl", "getLine").is_some());
        assert!(score("xyz", "getLine").is_none());
        assert!(score("gl", "getLine") > score("gl", "angle"));
        assert!(score("len", "length") > score("len", "is_lenient"));
        assert!(score("GETL", "getLine").is_some());
    }

    #[test]
    fn an_empty_fuzzy_query_matches_everything() {
        assert_eq!(FuzzyQuery::new("").score("anything"), Some(0));
    }

    #[test]
    fn formatting_edits_keep_the_cursor_on_its_text() {
        let text = Text::new("fn  main(){}");
        let edits = [
            TextEdit { range: Range::new(Position::new(0, 2), Position::new(0, 4)), new_text: " ".into() },
            TextEdit { range: Range::new(Position::new(0, 10), Position::new(0, 10)), new_text: " ".into() },
        ];
        let cursor = crate::selection::Selections::cursor(6); // In `main`.

        let transaction = transaction_keeping_selections(&text, &edits, &cursor).unwrap();

        assert_eq!(transaction.changes.apply(&text).to_string(), "fn main() {}");
        assert_eq!(transaction.selections, crate::selection::Selections::cursor(5));
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
