//! Go to Symbol (Ctrl+Shift+O): the document's definitions from the syntax tree, filtered by
//! the query; Enter selects the symbol's name.

use egui::{Event, Key, Modifiers, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rustext_core::EditorState;
use rustext_egui::{CodeEditor, EditorTheme, EditorView, Keymap, ViewConfig};
use rustext_syntax::{LanguageRegistry, SyntaxHighlighter, SyntaxTheme};

const SOURCE: &str = "struct Point;\nimpl Point {\n    fn length(&self) {}\n}\nfn main() {}\n";

struct Fixture {
    state: EditorState,
    view: EditorView,
    config: ViewConfig,
    theme: EditorTheme,
    keymap: Keymap,
    highlighter: SyntaxHighlighter,
}

fn harness() -> Harness<'static, Fixture> {
    let languages = LanguageRegistry::new();
    let mut fixture = Fixture {
        state: EditorState::new(SOURCE),
        view: EditorView::default(),
        config: ViewConfig::default(),
        theme: EditorTheme::dark(),
        keymap: Keymap::vscode(),
        highlighter: SyntaxHighlighter::new(&languages, languages.detect("main.rs", ""), SyntaxTheme::dark_modern()),
    };
    fixture.highlighter.wait_until_parsed(fixture.state.text());
    let mut harness = Harness::builder().with_size(Vec2::new(800.0, 400.0)).build_ui_state(
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
            if !fixture.view.palette.is_open {
                response.response.request_focus();
            }
        },
        fixture,
    );
    harness.run();
    harness
}

#[test]
fn ctrl_shift_o_lists_symbols_and_enter_selects_one() {
    let mut harness = harness();

    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::O);
    harness.run();
    harness.get_by_label("length");
    harness.get_by_label("main");
    harness.event(Event::Text("len".into()));
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();

    let state = &harness.state().state;
    let primary = state.selections().primary();
    assert_eq!(state.text().slice(primary.start(), primary.end()), "length");
    assert!(!harness.state().view.palette.is_open);
}
