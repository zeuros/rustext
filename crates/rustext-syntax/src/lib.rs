//! Languages, token themes and incremental syntax highlighting for the rustext editor:
//! tree-sitter grammars where one is bundled (DECISIONS D22), else syntect with the TextMate
//! grammars two-face bundles from bat (D13).

pub mod highlighter;
pub mod language;
pub mod textmate;
pub mod theme;
pub mod tree;

pub use highlighter::SyntaxHighlighter;
pub use language::{Language, LanguageRegistry};
pub use theme::{SyntaxTheme, ThemeColors};
