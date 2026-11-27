//! Brackets from the syntax tree: with a tree-sitter grammar, matching knows what the lexer
//! (D21) doesn't, such as Rust's nested comments.

use egui::{Key, Modifiers, Vec2};
use egui_kittest::Harness;
use rustext_core::{Command, EditorState, Position, Selections};
use rustext_egui::{CodeEditor, EditorTheme, EditorView, Keymap, ViewConfig};
use rustext_syntax::{LanguageRegistry, SyntaxHighlighter, SyntaxTheme};

struct Fixture {
    state: EditorState,
    view: EditorView,
    config: ViewConfig,
    theme: EditorTheme,
    keymap: Keymap,
    highlighter: SyntaxHighlighter,
}

fn rust_harness(source: &str) -> Harness<'static, Fixture> {
    let languages = LanguageRegistry::new();
    let language = languages.detect("a.rs", "");
    let state = EditorState::new(source).with_language(language.configuration.clone());
    let mut fixture = Fixture {
        state,
        view: EditorView::default(),
        config: ViewConfig::default(),
        theme: EditorTheme::dark(),
        keymap: Keymap::vscode(),
        highlighter: SyntaxHighlighter::new(&languages, language, SyntaxTheme::dark_modern()),
    };
    fixture.highlighter.wait_until_parsed(fixture.state.text());
    let mut harness = Harness::builder().with_size(Vec2::new(600.0, 200.0)).build_ui_state(
        |ui, fixture: &mut Fixture| {
            let response = CodeEditor::new(
                &mut fixture.state,
                &mut fixture.view,
                &fixture.config,
                &fixture.theme,
                &fixture.keymap,
            )
            .styler(&mut fixture.highlighter)
            .show(ui);
            response.response.request_focus();
        },
        fixture,
    );
    harness.run();
    harness
}

#[test]
fn jump_to_bracket_skips_a_bracket_in_a_nested_comment() {
    // The lexer ends the comment at the first `*/` and would take its `)` for code.
    let mut harness = rust_harness("f(/* /* */ ) */ x)");
    harness.state_mut().state.execute(Command::SetSelections(Selections::cursor(1)));

    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Backslash);
    harness.run();

    let state = &harness.state().state;
    assert_eq!(state.text().position(state.selections().primary().head), Position::new(0, 17));
}
