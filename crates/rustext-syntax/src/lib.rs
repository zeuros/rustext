//! Languages, token themes and incremental syntax highlighting for the rustext editor
//! (DECISIONS D13): syntect with the grammars two-face bundles from bat.

pub mod highlighter;
pub mod language;
pub mod theme;

pub use highlighter::SyntaxHighlighter;
pub use language::{Language, LanguageRegistry};
pub use theme::{SyntaxTheme, ThemeColors};
