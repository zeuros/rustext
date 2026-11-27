//! The bundled tree-sitter grammars and their queries: highlights, tags, bracket tokens and
//! embedded languages.

use super::*;

/// A compiled grammar: the language, its highlight query, its tags query, a query of its
/// bracket tokens and a query of the languages embedded in it.
pub struct Grammar {
    pub(super) language: tree_sitter::Language,
    pub(super) highlights: Query,
    pub(super) tags: Option<Query>,
    pub(super) brackets: Option<Query>,
    pub(super) injections: Option<Query>,
}

impl std::fmt::Debug for Grammar {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Grammar").field("captures", &self.highlights.capture_names().len()).finish()
    }
}

/// The bundled tree-sitter grammar of a language (by its syntect name): the language, its
/// highlight queries (the first winning where two capture the same node) and its tags query
/// (definitions, for Go to Symbol), when the grammar crate has one.
pub(super) fn grammar_source(language_name: &str) -> Option<(LanguageFn, Vec<&'static str>, Option<&'static str>)> {
    let javascript = tree_sitter_javascript::HIGHLIGHT_QUERY;
    let jsx = tree_sitter_javascript::JSX_HIGHLIGHT_QUERY;
    let typescript = tree_sitter_typescript::HIGHLIGHTS_QUERY;
    Some(match language_name {
        "Rust" => {
            (tree_sitter_rust::LANGUAGE, vec![tree_sitter_rust::HIGHLIGHTS_QUERY], Some(tree_sitter_rust::TAGS_QUERY))
        }
        "Python" => (
            tree_sitter_python::LANGUAGE,
            vec![tree_sitter_python::HIGHLIGHTS_QUERY],
            Some(tree_sitter_python::TAGS_QUERY),
        ),
        "JavaScript" | "JavaScript (Babel)" => {
            (tree_sitter_javascript::LANGUAGE, vec![jsx, javascript], Some(tree_sitter_javascript::TAGS_QUERY))
        }
        "TypeScript" => (
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
            vec![typescript, javascript],
            Some(tree_sitter_typescript::TAGS_QUERY),
        ),
        "TypeScriptReact" => (
            tree_sitter_typescript::LANGUAGE_TSX,
            vec![typescript, jsx, javascript],
            Some(tree_sitter_typescript::TAGS_QUERY),
        ),
        "JSON" => (tree_sitter_json::LANGUAGE, vec![tree_sitter_json::HIGHLIGHTS_QUERY], None),
        "Go" => (tree_sitter_go::LANGUAGE, vec![tree_sitter_go::HIGHLIGHTS_QUERY], Some(tree_sitter_go::TAGS_QUERY)),
        "C" => (tree_sitter_c::LANGUAGE, vec![tree_sitter_c::HIGHLIGHT_QUERY], Some(tree_sitter_c::TAGS_QUERY)),
        "C++" => (
            tree_sitter_cpp::LANGUAGE,
            vec![tree_sitter_cpp::HIGHLIGHT_QUERY, tree_sitter_c::HIGHLIGHT_QUERY],
            Some(tree_sitter_cpp::TAGS_QUERY),
        ),
        "Bourne Again Shell (bash)" | "Shell-Unix-Generic" => {
            (tree_sitter_bash::LANGUAGE, vec![tree_sitter_bash::HIGHLIGHT_QUERY], None)
        }
        "HTML" => (tree_sitter_html::LANGUAGE, vec![tree_sitter_html::HIGHLIGHTS_QUERY], None),
        "CSS" => (tree_sitter_css::LANGUAGE, vec![tree_sitter_css::HIGHLIGHTS_QUERY], None),
        "TOML" => (tree_sitter_toml_ng::LANGUAGE, vec![tree_sitter_toml_ng::HIGHLIGHTS_QUERY], None),
        "YAML" => (tree_sitter_yaml::LANGUAGE, vec![tree_sitter_yaml::HIGHLIGHTS_QUERY], None),
        "Java" => {
            (tree_sitter_java::LANGUAGE, vec![tree_sitter_java::HIGHLIGHTS_QUERY], Some(tree_sitter_java::TAGS_QUERY))
        }
        "C#" => (
            tree_sitter_c_sharp::LANGUAGE,
            vec![tree_sitter_c_sharp::HIGHLIGHTS_QUERY],
            Some(tree_sitter_c_sharp::TAGS_QUERY),
        ),
        "Ruby" => {
            (tree_sitter_ruby::LANGUAGE, vec![tree_sitter_ruby::HIGHLIGHTS_QUERY], Some(tree_sitter_ruby::TAGS_QUERY))
        }
        "Lua" => {
            (tree_sitter_lua::LANGUAGE, vec![tree_sitter_lua::HIGHLIGHTS_QUERY], Some(tree_sitter_lua::TAGS_QUERY))
        }
        _ => return None,
    })
}

/// The bundled query of the languages embedded in `language_name`'s text (DECISIONS D28).
pub(super) fn injections_source(language_name: &str) -> Option<&'static str> {
    match language_name {
        "HTML" => Some(tree_sitter_html::INJECTIONS_QUERY),
        _ => None,
    }
}

/// The bundled language an injection query names (`javascript`, `css`).
pub(super) fn injected_language_name(injection_language: &str) -> Option<&'static str> {
    TREE_SITTER_LANGUAGES.iter().copied().find(|name| name.eq_ignore_ascii_case(injection_language))
}

/// Names of the languages with a bundled tree-sitter grammar.
pub const TREE_SITTER_LANGUAGES: &[&str] = &[
    "Rust",
    "Python",
    "JavaScript",
    "TypeScript",
    "TypeScriptReact",
    "JSON",
    "Go",
    "C",
    "C++",
    "Bourne Again Shell (bash)",
    "HTML",
    "CSS",
    "TOML",
    "YAML",
    "Java",
    "C#",
    "Ruby",
    "Lua",
];

impl Grammar {
    /// Compiles the bundled grammar of `language_name`; `None` without one, or when its query
    /// doesn't compile (the TextMate grammar is used instead).
    pub(crate) fn compile(language_name: &str) -> Result<Option<Grammar>, String> {
        let Some((language_fn, queries, tags)) = grammar_source(language_name) else { return Ok(None) };
        let language = tree_sitter::Language::new(language_fn);
        let highlights = Query::new(&language, &queries.join("\n")).map_err(|error| error.to_string())?;
        let tags = tags.map(|tags| Query::new(&language, tags)).transpose().map_err(|error| error.to_string())?;
        let brackets = bracket_query(&language);
        let injections = injections_source(language_name)
            .map(|injections| Query::new(&language, injections))
            .transpose()
            .map_err(|error| error.to_string())?;
        Ok(Some(Grammar { language, highlights, tags, brackets, injections }))
    }
}

/// The chars that are brackets in some language; the language configuration picks its own.
pub(super) const BRACKET_CHARS: &str = "()[]{}";

/// A query capturing the bracket tokens of `language`: its anonymous node kinds of at most
/// three punctuation chars holding a bracket (`(`, `${`, `$((`, `[[`). Brackets in strings and
/// comments are inside other tokens, so they are not captured. A language without such tokens
/// (HTML) has its `text` nodes captured instead: brackets of prose still pair. `None` when it
/// has neither.
pub(super) fn bracket_query(language: &tree_sitter::Language) -> Option<Query> {
    let is_bracket_token = |kind: &str| {
        kind.len() <= 3
            && kind.chars().all(|character| character.is_ascii_punctuation() && !"\"\\".contains(character))
            && kind.chars().any(|character| BRACKET_CHARS.contains(character))
    };
    let kinds: BTreeSet<&str> = (0..language.node_kind_count() as u16)
        .filter(|&id| !language.node_kind_is_named(id) && language.node_kind_is_visible(id))
        .filter_map(|id| language.node_kind_for_id(id))
        .filter(|kind| is_bracket_token(kind))
        .collect();
    if kinds.is_empty() {
        let has_text_nodes = language.id_for_node_kind("text", true) != 0;
        return has_text_nodes.then(|| Query::new(language, "(text) @text").ok()).flatten();
    }
    let alternatives: Vec<String> = kinds.iter().map(|kind| format!("\"{kind}\"")).collect();
    Query::new(language, &format!("[{}] @bracket", alternatives.join(" "))).ok()
}

/// TextMate scopes for tree-sitter capture names; a capture uses its longest listed prefix
/// (`function.method.call` → `function.method`).
pub(super) const CAPTURE_SCOPES: &[(&str, &str)] = &[
    ("comment", "comment"),
    ("string", "string"),
    ("string.special.regex", "string.regexp"),
    ("string.escape", "constant.character.escape"),
    ("escape", "constant.character.escape"),
    ("character", "constant.character"),
    ("number", "constant.numeric"),
    ("float", "constant.numeric"),
    ("boolean", "constant.language"),
    ("constant.builtin", "constant.language"),
    ("constant", "variable.other.constant"),
    ("function.macro", "support.macro"),
    ("function.builtin", "support.function"),
    ("function", "entity.name.function"),
    ("method", "entity.name.function"),
    ("constructor", "entity.name.type"),
    ("type.builtin", "storage.type.primitive"),
    ("type", "entity.name.type"),
    ("keyword.function", "storage.type.function"),
    ("keyword.operator", "keyword.operator"),
    ("keyword.return", "keyword.control"),
    ("keyword.control", "keyword.control"),
    ("keyword", "keyword"),
    ("conditional", "keyword.control"),
    ("repeat", "keyword.control"),
    ("import", "keyword.control.import"),
    ("preproc", "keyword.control.directive"),
    ("charset", "keyword.control.at-rule"),
    ("media", "keyword.control.at-rule"),
    ("keyframes", "keyword.control.at-rule"),
    ("supports", "keyword.control.at-rule"),
    ("operator", "keyword.operator"),
    ("variable.parameter", "variable.parameter"),
    ("parameter", "variable.parameter"),
    ("variable.builtin", "variable.language"),
    ("variable", "variable"),
    ("property", "variable.other.property"),
    ("field", "variable.other.property"),
    ("attribute", "entity.other.attribute-name"),
    ("tag", "entity.name.tag"),
    ("label", "entity.name.label"),
    ("namespace", "entity.name.namespace"),
    ("module", "entity.name.namespace"),
    ("punctuation.delimiter", "punctuation.separator"),
    ("delimiter", "punctuation.separator"),
    ("punctuation", "punctuation"),
];

/// The TextMate scope of a capture name, if any prefix of it is listed.
pub(super) fn textmate_scope(capture_name: &str) -> Option<&'static str> {
    let mut name = capture_name;
    loop {
        if let Some(&(_, scope)) = CAPTURE_SCOPES.iter().find(|(capture, _)| *capture == name) {
            return Some(scope);
        }
        name = name.rsplit_once('.')?.0;
    }
}

/// The style of each capture of `query` in `theme`; `None` for the default style.
pub(super) fn capture_styles(query: &Query, theme: &SyntaxTheme) -> Vec<Option<TextStyle>> {
    let highlighter = Highlighter::new(&theme.theme);
    let default_style = highlighter.get_default();
    query
        .capture_names()
        .iter()
        .map(|name| {
            let scope = Scope::new(textmate_scope(name)?).ok()?;
            let style = highlighter.style_for_stack(&[scope]);
            (style != default_style).then(|| text_style(style, default_style))
        })
        .collect()
}
