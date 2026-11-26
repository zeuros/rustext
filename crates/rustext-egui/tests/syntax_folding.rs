//! Folding by syntax: with a tree-sitter grammar, blocks fold by structure, whatever their
//! indentation; `FoldingStrategy::Indentation` goes back to indentation.

use egui::{Event, Key, Modifiers, Vec2};
use egui_kittest::Harness;
use rustext_core::{Command, EditorState, Selections};
use rustext_egui::code_editor::FoldingStrategy;
use rustext_egui::{CodeEditor, EditorTheme, EditorView, Keymap, ViewConfig};
use rustext_syntax::{LanguageRegistry, SyntaxHighlighter, SyntaxTheme};

/// Unindented JSON: indentation finds no block, the syntax tree finds two.
const JSON: &str = "{\n\"a\": [\n1,\n2\n]\n}\n";

struct Fixture {
    state: EditorState,
    view: EditorView,
    config: ViewConfig,
    theme: EditorTheme,
    keymap: Keymap,
    highlighter: SyntaxHighlighter,
}

fn harness(folding_strategy: FoldingStrategy) -> Harness<'static, Fixture> {
    let languages = LanguageRegistry::new();
    let mut state = EditorState::new(JSON);
    state.execute(Command::SetSelections(Selections::cursor(state.text().line_start(1))));
    let fixture = Fixture {
        state,
        view: EditorView::default(),
        config: ViewConfig { folding_strategy, ..ViewConfig::default() },
        theme: EditorTheme::dark(),
        keymap: Keymap::vscode(),
        highlighter: SyntaxHighlighter::new(&languages, languages.detect("a.json", ""), SyntaxTheme::dark_modern()),
    };
    let mut harness = Harness::builder().with_size(Vec2::new(600.0, 300.0)).build_ui_state(
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

fn fold_at_cursor(harness: &mut Harness<'_, Fixture>) {
    harness.event(Event::Key {
        key: Key::OpenBracket,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND | Modifiers::SHIFT,
    });
    harness.run();
}

#[test]
fn unindented_blocks_fold_by_syntax() {
    let mut harness = harness(FoldingStrategy::Auto);

    fold_at_cursor(&mut harness);

    assert_eq!(harness.state().view.folds.folded_header_lines(), vec![1]);
}

#[test]
fn folding_from_inside_a_block_moves_the_cursor_to_its_header() {
    let mut harness = harness(FoldingStrategy::Auto);
    let line_two = harness.state().state.text().line_start(2);
    harness.state_mut().state.execute(Command::SetSelections(Selections::cursor(line_two)));

    fold_at_cursor(&mut harness);

    let state = &harness.state().state;
    assert_eq!(harness.state().view.folds.folded_header_lines(), vec![1]);
    assert_eq!(state.selections(), &Selections::cursor(state.text().line_content_end(1)));
}

#[test]
fn the_indentation_strategy_ignores_the_syntax_tree() {
    let mut harness = harness(FoldingStrategy::Indentation);

    fold_at_cursor(&mut harness);

    assert!(harness.state().view.folds.folded_header_lines().is_empty());
}
