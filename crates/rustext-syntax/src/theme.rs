//! Token color themes: scope rules (TextMate selectors) to styles. Built-in "Dark Modern" and
//! "Light Modern" approximate VS Code's; any two-face or `.tmTheme` theme loads too.

use std::io::Cursor;
use std::str::FromStr;
use std::sync::Arc;

use rustext_core::style::Rgba;
use syntect::highlighting::{Color, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings};

pub use two_face::theme::EmbeddedThemeName;

/// A token theme. Cloning shares it.
#[derive(Clone, Debug)]
pub struct SyntaxTheme {
    pub(crate) theme: Arc<Theme>,
}

/// The editor colors a token theme defines, when it does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThemeColors {
    pub background: Option<Rgba>,
    pub foreground: Option<Rgba>,
    pub selection: Option<Rgba>,
    pub current_line: Option<Rgba>,
    pub cursor: Option<Rgba>,
    pub line_number: Option<Rgba>,
}

#[derive(Clone, Copy)]
struct Rule {
    selectors: &'static str,
    color: u32,
    font_style: FontStyle,
}

const fn rule(selectors: &'static str, color: u32) -> Rule {
    Rule { selectors, color, font_style: FontStyle::empty() }
}

const fn styled(selectors: &'static str, color: u32, font_style: FontStyle) -> Rule {
    Rule { selectors, color, font_style }
}

const DARK_MODERN_RULES: &[Rule] = &[
    rule("comment, punctuation.definition.comment", 0x6a9955),
    rule("string, punctuation.definition.string", 0xce9178),
    rule("string.regexp", 0xd16969),
    rule("constant.character.escape", 0xd7ba7d),
    rule("constant.numeric", 0xb5cea8),
    rule("constant.language, constant.other.boolean", 0x569cd6),
    rule("constant.other, variable.other.constant", 0x4fc1ff),
    rule("keyword, storage.type, storage.modifier", 0x569cd6),
    rule("keyword.control, keyword.other.use, keyword.other.import, keyword.control.import", 0xc586c0),
    rule("keyword.operator, punctuation.separator, punctuation.terminator, punctuation.accessor", 0xd4d4d4),
    rule("entity.name.function, support.function, meta.function-call entity.name, variable.function", 0xdcdcaa),
    rule("support.macro, entity.name.macro, meta.macro", 0xdcdcaa),
    rule(
        "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, entity.name.trait, entity.name.interface, entity.name.namespace, entity.name.module, support.type, support.class, storage.type.primitive",
        0x4ec9b0,
    ),
    rule("variable, variable.parameter, entity.name.variable, meta.definition.variable", 0x9cdcfe),
    rule("variable.language, variable.language.this, variable.language.self", 0x569cd6),
    rule("entity.name.tag", 0x569cd6),
    rule("entity.other.attribute-name, meta.attribute", 0x9cdcfe),
    rule("entity.name.label, punctuation.definition.label", 0xc8c8c8),
    rule("meta.annotation, punctuation.definition.annotation, entity.name.decorator", 0xdcdcaa),
    styled("markup.heading, entity.name.section", 0x569cd6, FontStyle::BOLD),
    styled("markup.bold", 0xcccccc, FontStyle::BOLD),
    styled("markup.italic", 0xcccccc, FontStyle::ITALIC),
    styled("markup.underline.link, string.other.link", 0x3794ff, FontStyle::UNDERLINE),
    rule("markup.inline.raw, markup.raw", 0xce9178),
    rule("markup.inserted", 0xb5cea8),
    rule("markup.deleted", 0xce9178),
    rule("markup.changed", 0x569cd6),
    rule("invalid", 0xf44747),
];

const LIGHT_MODERN_RULES: &[Rule] = &[
    rule("comment, punctuation.definition.comment", 0x008000),
    rule("string, punctuation.definition.string", 0xa31515),
    rule("string.regexp", 0x811f3f),
    rule("constant.character.escape", 0xee0000),
    rule("constant.numeric", 0x098658),
    rule("constant.language, constant.other.boolean", 0x0000ff),
    rule("constant.other, variable.other.constant", 0x0070c1),
    rule("keyword, storage.type, storage.modifier", 0x0000ff),
    rule("keyword.control, keyword.other.use, keyword.other.import, keyword.control.import", 0xaf00db),
    rule("keyword.operator, punctuation.separator, punctuation.terminator, punctuation.accessor", 0x000000),
    rule("entity.name.function, support.function, meta.function-call entity.name, variable.function", 0x795e26),
    rule("support.macro, entity.name.macro, meta.macro", 0x795e26),
    rule(
        "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, entity.name.trait, entity.name.interface, entity.name.namespace, entity.name.module, support.type, support.class, storage.type.primitive",
        0x267f99,
    ),
    rule("variable, variable.parameter, entity.name.variable, meta.definition.variable", 0x001080),
    rule("variable.language, variable.language.this, variable.language.self", 0x0000ff),
    rule("entity.name.tag", 0x800000),
    rule("entity.other.attribute-name, meta.attribute", 0xe50000),
    rule("meta.annotation, punctuation.definition.annotation, entity.name.decorator", 0x795e26),
    styled("markup.heading, entity.name.section", 0x800000, FontStyle::BOLD),
    styled("markup.bold", 0x000080, FontStyle::BOLD),
    styled("markup.italic", 0x3b3b3b, FontStyle::ITALIC),
    styled("markup.underline.link, string.other.link", 0x0451a5, FontStyle::UNDERLINE),
    rule("markup.inline.raw, markup.raw", 0x800000),
    rule("markup.inserted", 0x098658),
    rule("markup.deleted", 0xa31515),
    rule("markup.changed", 0x0451a5),
    rule("invalid", 0xcd3131),
];

fn color(rgb: u32) -> Color {
    Color { r: (rgb >> 16) as u8, g: (rgb >> 8) as u8, b: rgb as u8, a: 0xff }
}

pub(crate) fn to_rgba(color: Color) -> Rgba {
    Rgba { red: color.r, green: color.g, blue: color.b, alpha: color.a }
}

fn built_theme(name: &str, foreground: u32, background: u32, rules: &[Rule]) -> Theme {
    let scopes = rules
        .iter()
        .map(|rule| ThemeItem {
            scope: ScopeSelectors::from_str(rule.selectors).expect("valid built-in scope selectors"),
            style: StyleModifier {
                foreground: Some(color(rule.color)),
                background: None,
                font_style: (!rule.font_style.is_empty()).then_some(rule.font_style),
            },
        })
        .collect();
    Theme {
        name: Some(name.to_string()),
        author: None,
        settings: ThemeSettings {
            foreground: Some(color(foreground)),
            background: Some(color(background)),
            ..ThemeSettings::default()
        },
        scopes,
    }
}

impl SyntaxTheme {
    pub fn dark_modern() -> Self {
        Self { theme: Arc::new(built_theme("Dark Modern", 0xcccccc, 0x1f1f1f, DARK_MODERN_RULES)) }
    }

    pub fn light_modern() -> Self {
        Self { theme: Arc::new(built_theme("Light Modern", 0x3b3b3b, 0xffffff, LIGHT_MODERN_RULES)) }
    }

    /// One of bat's themes bundled by two-face (Dracula, Nord, Solarized, Gruvbox…).
    pub fn embedded(name: EmbeddedThemeName) -> Self {
        Self { theme: Arc::new(two_face::theme::extra().get(name).clone()) }
    }

    /// A TextMate `.tmTheme` (XML plist) theme.
    pub fn from_tm_theme(content: &str) -> Result<Self, String> {
        let theme = syntect::highlighting::ThemeSet::load_from_reader(&mut Cursor::new(content))
            .map_err(|error| error.to_string())?;
        Ok(Self { theme: Arc::new(theme) })
    }

    pub fn name(&self) -> &str {
        self.theme.name.as_deref().unwrap_or("Unnamed")
    }

    pub fn colors(&self) -> ThemeColors {
        let settings = &self.theme.settings;
        ThemeColors {
            background: settings.background.map(to_rgba),
            foreground: settings.foreground.map(to_rgba),
            selection: settings.selection.map(to_rgba),
            current_line: settings.line_highlight.map(to_rgba),
            cursor: settings.caret.map(to_rgba),
            line_number: settings.gutter_foreground.map(to_rgba),
        }
    }
}
