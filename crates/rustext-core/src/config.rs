//! Editing options: small typed structs with defaults (DECISIONS D12).

use crate::indentation::Indentation;
use crate::words::WordDefinition;

/// How a paste with as many lines as there are cursors is spread (Monaco's `multiCursorPaste`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MultiCursorPaste {
    /// One line per cursor.
    #[default]
    Spread,
    /// The whole text at every cursor.
    Full,
}

/// When to insert a closing bracket or quote after typing an opening one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AutoClosing {
    Always,
    /// Only when the char after the cursor is whitespace, a closing bracket, or the line end.
    #[default]
    BeforeWhitespace,
    Never,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditingConfig {
    pub indentation: Indentation,
    pub words: WordDefinition,
    pub read_only: bool,
    /// Copy / cut with an empty selection takes the whole line.
    pub empty_selection_clipboard: bool,
    pub multi_cursor_paste: MultiCursorPaste,
    /// Backspace in leading spaces deletes back to the previous tab stop.
    pub use_tab_stops: bool,
    pub auto_closing_brackets: AutoClosing,
    pub auto_closing_quotes: AutoClosing,
    /// Typing a closing char that was auto-inserted steps over it.
    pub auto_closing_overtype: bool,
    /// Backspace between an auto-closed pair deletes both.
    pub auto_closing_delete: bool,
    /// Typing a bracket or quote with a selection wraps the selection.
    pub auto_surround: bool,
    /// Enter keeps the indentation and indents after an opening bracket.
    pub auto_indent: bool,
    pub multi_cursor_limit: usize,
}

impl Default for EditingConfig {
    fn default() -> Self {
        Self {
            indentation: Indentation::default(),
            words: WordDefinition::default(),
            read_only: false,
            empty_selection_clipboard: true,
            multi_cursor_paste: MultiCursorPaste::default(),
            use_tab_stops: true,
            auto_closing_brackets: AutoClosing::default(),
            auto_closing_quotes: AutoClosing::default(),
            auto_closing_overtype: true,
            auto_closing_delete: true,
            auto_surround: true,
            auto_indent: true,
            multi_cursor_limit: 10_000,
        }
    }
}

/// What a language tells the editor (Monaco's `LanguageConfiguration`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageConfiguration {
    pub line_comment: Option<String>,
    pub block_comment: Option<(String, String)>,
    pub brackets: Vec<(char, char)>,
    pub auto_closing_pairs: Vec<AutoClosingPair>,
    pub surrounding_pairs: Vec<(char, char)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoClosingPair {
    pub open: char,
    pub close: char,
    /// Not auto-closed inside strings (e.g. `'` in prose-like contexts).
    pub not_in_string: bool,
}

impl AutoClosingPair {
    pub const fn new(open: char, close: char) -> Self {
        Self { open, close, not_in_string: false }
    }

    pub fn is_quote(&self) -> bool {
        self.open == self.close
    }
}

impl LanguageConfiguration {
    /// Brackets and quotes common to C-like languages, `//` and `/* */` comments.
    pub fn c_like() -> Self {
        Self { line_comment: Some("//".into()), block_comment: Some(("/*".into(), "*/".into())), ..Self::plain_text() }
    }

    /// Brackets and quotes, no comments.
    pub fn plain_text() -> Self {
        let brackets = vec![('(', ')'), ('[', ']'), ('{', '}')];
        let quotes = [('"', '"'), ('\'', '\''), ('`', '`')];
        Self {
            line_comment: None,
            block_comment: None,
            auto_closing_pairs: brackets
                .iter()
                .chain(quotes.iter())
                .map(|&(open, close)| AutoClosingPair::new(open, close))
                .collect(),
            surrounding_pairs: brackets.iter().copied().chain(quotes).collect(),
            brackets,
        }
    }

    pub fn with_line_comment(self, line_comment: &str) -> Self {
        Self { line_comment: Some(line_comment.into()), ..self }
    }

    pub fn with_block_comment(self, open: &str, close: &str) -> Self {
        Self { block_comment: Some((open.into(), close.into())), ..self }
    }

    pub fn without_block_comment(self) -> Self {
        Self { block_comment: None, ..self }
    }

    pub fn closing_bracket_of(&self, open: char) -> Option<char> {
        self.brackets.iter().find(|(bracket_open, _)| *bracket_open == open).map(|(_, close)| *close)
    }

    pub fn is_closing_bracket(&self, character: char) -> bool {
        self.brackets.iter().any(|(_, close)| *close == character)
    }
}

impl Default for LanguageConfiguration {
    fn default() -> Self {
        Self::plain_text()
    }
}
