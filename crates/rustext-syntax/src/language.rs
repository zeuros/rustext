//! Languages: detecting them from a file name, and what each tells the editor (comments).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rustext_core::config::LanguageConfiguration;
use syntect::parsing::{SyntaxReference, SyntaxSet};

use crate::tree::Grammar;

/// A language: its grammar and its editing configuration.
#[derive(Clone, Debug)]
pub struct Language {
    pub name: String,
    pub configuration: LanguageConfiguration,
    pub(crate) syntax: SyntaxReference,
}

impl Language {
    pub fn is_plain_text(&self) -> bool {
        self.name == "Plain Text"
    }
}

/// The known languages: bat's grammars bundled by two-face. A value passed around, not a
/// global (DECISIONS D6); cloning shares the grammars.
#[derive(Clone)]
pub struct LanguageRegistry {
    pub(crate) syntaxes: Arc<SyntaxSet>,
    /// Tree-sitter grammars compiled so far, by language name (`None`: no bundled grammar).
    grammars: Arc<Mutex<HashMap<String, Option<Arc<Grammar>>>>>,
}

impl LanguageRegistry {
    /// Loads the bundled grammars (~200 languages). Takes a few milliseconds: do it once.
    pub fn new() -> Self {
        Self { syntaxes: Arc::new(two_face::syntax::extra_newlines()), grammars: Default::default() }
    }

    pub fn plain_text(&self) -> Language {
        self.language(self.syntaxes.find_syntax_plain_text())
    }

    /// Finds the language of a file from its name (`Makefile`, `main.rs`), else from its first
    /// line (`#!/usr/bin/env python`), else plain text.
    pub fn detect(&self, file_name: &str, first_line: &str) -> Language {
        let base_name = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
        let extension = base_name.rsplit_once('.').map(|(_, extension)| extension);
        let syntax = self
            .syntaxes
            .find_syntax_by_extension(base_name)
            .or_else(|| extension.and_then(|extension| self.syntaxes.find_syntax_by_extension(extension)))
            .or_else(|| {
                extension.and_then(|extension| self.syntaxes.find_syntax_by_extension(&extension.to_lowercase()))
            })
            .or_else(|| self.syntaxes.find_syntax_by_first_line(first_line))
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text());
        self.language(syntax)
    }

    pub fn by_name(&self, name: &str) -> Option<Language> {
        self.syntaxes.find_syntax_by_name(name).map(|syntax| self.language(syntax))
    }

    /// Names of every known language, sorted.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.syntaxes.syntaxes().iter().map(|syntax| syntax.name.clone()).collect();
        names.sort_unstable_by_key(|name| name.to_lowercase());
        names.dedup();
        names
    }

    /// The tree-sitter grammar of `language_name`, compiled on first use (a few milliseconds);
    /// `None` when none is bundled or its queries don't compile.
    pub(crate) fn grammar(&self, language_name: &str) -> Option<Arc<Grammar>> {
        let mut grammars = self.grammars.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        grammars
            .entry(language_name.to_string())
            .or_insert_with(|| Grammar::compile(language_name).ok().flatten().map(Arc::new))
            .clone()
    }

    fn language(&self, syntax: &SyntaxReference) -> Language {
        Language { name: syntax.name.clone(), configuration: configuration_for(&syntax.name), syntax: syntax.clone() }
    }
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Comment tokens by language name. Brackets and quotes are the common ones for all.
pub fn configuration_for(language_name: &str) -> LanguageConfiguration {
    let base = LanguageConfiguration::plain_text();
    let c_like = LanguageConfiguration::c_like();
    let both_quotes = ['"', '\''];
    match language_name {
        // `'` opens strings here, not char literals.
        "JavaScript" | "JavaScript (Babel)" | "TypeScript" | "TypeScriptReact" | "PHP" | "Dart" | "Groovy" | "SCSS"
        | "Less" | "Sass" => c_like.with_string_quotes(&['"', '\'', '`']),
        "Rust" | "C" | "C++" | "C#" | "Java" | "Go" | "Swift" | "Kotlin" | "Scala" | "Objective-C"
        | "Objective-C++" | "Zig" | "JSON" | "Protocol Buffer" | "GLSL" | "Solidity" | "D" | "Verilog" | "Odin" => {
            c_like
        }
        "CSS" => base.with_block_comment("/*", "*/").with_string_quotes(&both_quotes),
        "Python"
        | "Ruby"
        | "Perl"
        | "R"
        | "Bourne Again Shell (bash)"
        | "Shell-Unix-Generic"
        | "fish"
        | "YAML"
        | "TOML"
        | "Makefile"
        | "Dockerfile"
        | "CMake"
        | "Nix"
        | "Elixir"
        | "Julia"
        | "PowerShell"
        | "Git Config"
        | "Git Ignore"
        | "Git Attributes"
        | "Crystal"
        | "Nim"
        | "GraphQL"
        | "Terraform"
        | "HCL"
        | "INI"
        | "Properties"
        | "nginx"
        | "Apache Conf"
        | "Puppet"
        | "Tcl"
        | "AWK" => base.with_line_comment("#").with_string_quotes(&both_quotes),
        "SQL" | "Lua" => base.with_line_comment("--").with_string_quotes(&both_quotes),
        "Haskell" | "Elm" | "Ada" | "VHDL" | "PureScript" => {
            LanguageConfiguration { char_quote: Some('\''), ..base.with_line_comment("--").with_string_quotes(&['"']) }
        }
        "Lisp" | "Clojure" | "Scheme" | "Racket" | "Assembly (x86_64)" | "x86_64 Assembly" => {
            base.with_line_comment(";").with_string_quotes(&['"'])
        }
        "Erlang" | "LaTeX" | "TeX" | "MATLAB" => base.with_line_comment("%"),
        "VimL" => base.with_line_comment("\""),
        "Batch File" => base.with_line_comment("REM"),
        "HTML" | "XML" | "Markdown" | "SVG" | "Vue Component" | "Svelte" | "Twig" | "Handlebars" => {
            base.with_block_comment("<!--", "-->")
        }
        "OCaml" | "F#" | "Pascal" | "Mathematica" => base.with_block_comment("(*", "*)"),
        _ => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_extension_file_name_and_first_line() {
        let registry = LanguageRegistry::new();

        assert_eq!(registry.detect("src/main.rs", "").name, "Rust");
        assert_eq!(registry.detect("app.ts", "").name, "TypeScript");
        assert_eq!(registry.detect("Cargo.toml", "").name, "TOML");
        assert_eq!(registry.detect("Makefile", "").name, "Makefile");
        assert_eq!(registry.detect("script", "#!/usr/bin/env python3").name, "Python");
        assert!(registry.detect("notes.unknown-extension", "hello").is_plain_text());
    }

    #[test]
    fn languages_know_their_comments() {
        let registry = LanguageRegistry::new();

        assert_eq!(registry.detect("a.py", "").configuration.line_comment.as_deref(), Some("#"));
        assert_eq!(registry.detect("a.rs", "").configuration.line_comment.as_deref(), Some("//"));
        assert_eq!(registry.detect("a.html", "").configuration.block_comment, Some(("<!--".into(), "-->".into())));
    }

    #[test]
    fn languages_know_their_string_quotes() {
        let registry = LanguageRegistry::new();

        let rust = registry.detect("a.rs", "").configuration;
        assert_eq!((rust.string_quotes.contains(&'\''), rust.char_quote), (false, Some('\'')));
        let typescript = registry.detect("a.ts", "").configuration;
        assert_eq!((typescript.string_quotes.contains(&'\''), typescript.char_quote), (true, None));
        assert!(registry.detect("notes.unknown-extension", "").configuration.string_quotes.is_empty());
    }
}
