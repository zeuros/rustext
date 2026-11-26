//! Languages: detecting them from a file name, and what each tells the editor (comments).

use std::sync::Arc;

use rustext_core::config::LanguageConfiguration;
use syntect::parsing::{SyntaxReference, SyntaxSet};

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
}

impl LanguageRegistry {
    /// Loads the bundled grammars (~200 languages). Takes a few milliseconds: do it once.
    pub fn new() -> Self {
        Self { syntaxes: Arc::new(two_face::syntax::extra_newlines()) }
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
    match language_name {
        "Rust" | "C" | "C++" | "C#" | "Java" | "JavaScript" | "JavaScript (Babel)" | "TypeScript"
        | "TypeScriptReact" | "Go" | "Swift" | "Kotlin" | "Scala" | "Dart" | "PHP" | "Objective-C"
        | "Objective-C++" | "Groovy" | "Zig" | "JSON" | "Protocol Buffer" | "GLSL" | "Solidity" | "D" | "SCSS"
        | "Less" | "Sass" | "Verilog" | "Odin" => c_like,
        "CSS" => base.with_block_comment("/*", "*/"),
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
        | "AWK" => base.with_line_comment("#"),
        "SQL" | "Lua" | "Haskell" | "Elm" | "Ada" | "VHDL" | "PureScript" => base.with_line_comment("--"),
        "Lisp" | "Clojure" | "Scheme" | "Racket" | "Assembly (x86_64)" | "x86_64 Assembly" => {
            base.with_line_comment(";")
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
}
