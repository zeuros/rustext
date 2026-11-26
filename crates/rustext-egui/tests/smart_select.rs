//! Smart select: Shift+Alt+Right grows the selection by syntax node when the styler parses the
//! text, else by word, line and whole text; Shift+Alt+Left goes back.

use egui::{Event, Key, Modifiers, Vec2};
use egui_kittest::Harness;
use rustext_core::{EditorState, Selection, Selections};
use rustext_egui::{CodeEditor, EditorTheme, EditorView, Keymap, ViewConfig};
use rustext_syntax::{LanguageRegistry, SyntaxHighlighter, SyntaxTheme};

struct Fixture {
    state: EditorState,
    view: EditorView,
    config: ViewConfig,
    theme: EditorTheme,
    keymap: Keymap,
    highlighter: Option<SyntaxHighlighter>,
}

fn harness(content: &str, cursor: usize, file_name: Option<&str>) -> Harness<'static, Fixture> {
    let languages = LanguageRegistry::new();
    let highlighter = file_name.map(|file_name| {
        SyntaxHighlighter::new(&languages, languages.detect(file_name, ""), SyntaxTheme::dark_modern())
    });
    let mut state = EditorState::new(content);
    state.execute(rustext_core::Command::SetSelections(Selections::cursor(cursor)));
    let mut fixture = Fixture {
        state,
        view: EditorView::default(),
        config: ViewConfig::default(),
        theme: EditorTheme::dark(),
        keymap: Keymap::vscode(),
        highlighter,
    };
    if let Some(highlighter) = fixture.highlighter.as_mut() {
        highlighter.wait_until_parsed(fixture.state.text());
    }
    let mut harness = Harness::builder().with_size(Vec2::new(800.0, 400.0)).build_ui_state(
        |ui, fixture: &mut Fixture| {
            let editor = CodeEditor::new(
                &mut fixture.state,
                &mut fixture.view,
                &fixture.config,
                &fixture.theme,
                &fixture.keymap,
            );
            let editor = match fixture.highlighter.as_mut() {
                Some(highlighter) => editor.styler(highlighter),
                None => editor,
            };
            let response = editor.show(ui);
            response.response.request_focus();
        },
        fixture,
    );
    harness.run();
    harness
}

fn press(harness: &mut Harness<'_, Fixture>, key: Key) {
    harness.event(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::SHIFT | Modifiers::ALT,
    });
    harness.run();
}

fn selected(harness: &Harness<'_, Fixture>) -> String {
    let state = &harness.state().state;
    let primary = state.selections().primary();
    state.text().slice(primary.start(), primary.end())
}

#[test]
fn shift_alt_right_grows_the_selection_node_by_node() {
    let mut harness = harness("fn f() { g(1 + 2); }", 11, Some("main.rs"));

    press(&mut harness, Key::ArrowRight);
    assert_eq!(selected(&harness), "1");
    press(&mut harness, Key::ArrowRight);
    assert_eq!(selected(&harness), "1 + 2");
    press(&mut harness, Key::ArrowRight);
    assert_eq!(selected(&harness), "(1 + 2)");
}

#[test]
fn shift_alt_left_goes_back_to_where_the_growth_started() {
    let mut harness = harness("fn f() { g(1 + 2); }", 11, Some("main.rs"));
    press(&mut harness, Key::ArrowRight);
    press(&mut harness, Key::ArrowRight);

    press(&mut harness, Key::ArrowLeft);
    assert_eq!(selected(&harness), "1");
    press(&mut harness, Key::ArrowLeft);
    assert_eq!(harness.state().state.selections(), &Selections::cursor(11));
}

#[test]
fn without_a_syntax_tree_the_selection_grows_by_word_then_line() {
    let mut harness = harness("first line\nsecond words here\n", 15, None);

    press(&mut harness, Key::ArrowRight);
    assert_eq!(selected(&harness), "second");
    press(&mut harness, Key::ArrowRight);
    assert_eq!(selected(&harness), "second words here");
}

#[test]
fn moving_the_cursor_forgets_what_to_shrink_back_to() {
    let mut harness = harness("fn f() { g(1 + 2); }", 11, Some("main.rs"));
    press(&mut harness, Key::ArrowRight);
    harness.state_mut().state.execute(rustext_core::Command::SetSelections(Selections::single(Selection::new(3, 4))));

    press(&mut harness, Key::ArrowLeft);
    assert_eq!(selected(&harness), "f");
}
