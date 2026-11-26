//! Drives completion, hover and go-to-definition through a test `LanguageServices`.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rustext_core::services::{CompletionItem, CompletionKind, LanguageServices, Reply, WordCompletions};
use rustext_core::{EditorState, Position, Text};
use rustext_egui::{CodeEditor, EditorTheme, EditorView, Keymap, ViewConfig};

/// Word completions, plus a hover text and a definition for `helper`.
struct TestServices {
    words: WordCompletions,
}

impl LanguageServices for TestServices {
    fn completions(&mut self, text: &Text, revision: u64, position: Position) -> Reply<Vec<CompletionItem>> {
        let Reply::Ready(mut items) = self.words.completions(text, revision, position) else { return Reply::Pending };
        items.push(CompletionItem::new("helper", CompletionKind::Function).with_detail("fn() -> u8"));
        Reply::Ready(items)
    }

    fn completion_trigger_chars(&self) -> &[char] {
        &['.']
    }

    fn hover(&mut self, text: &Text, _revision: u64, position: Position) -> Reply<Option<String>> {
        let line: Vec<char> = text.line_content(position.line).chars().collect();
        let on_helper = WordCompletions::default()
            .words
            .word_at(&line, position.column)
            .is_some_and(|(start, end)| line[start..end].iter().collect::<String>() == "helper");
        Reply::Ready(on_helper.then(|| "fn helper() -> u8".to_string()))
    }

    fn definition(&mut self, _text: &Text, _revision: u64, _position: Position) -> Reply<Option<Position>> {
        Reply::Ready(Some(Position::new(0, 3)))
    }
}

struct Fixture {
    state: EditorState,
    view: EditorView,
    config: ViewConfig,
    theme: EditorTheme,
    keymap: Keymap,
    services: TestServices,
}

fn harness(content: &str) -> Harness<'static, Fixture> {
    let fixture = Fixture {
        state: EditorState::new(content),
        view: EditorView::default(),
        config: ViewConfig { cursor_blinking: false, ..ViewConfig::default() },
        theme: EditorTheme::dark(),
        keymap: Keymap::vscode(),
        services: TestServices { words: WordCompletions::default() },
    };
    let mut harness = Harness::builder().with_size(Vec2::new(900.0, 500.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, fixture: &mut Fixture| {
            CodeEditor::new(&mut fixture.state, &mut fixture.view, &fixture.config, &fixture.theme, &fixture.keymap)
                .services(&mut fixture.services)
                .show(ui);
        },
        fixture,
    );
    harness.run();
    harness
}

fn press(harness: &mut Harness<'_, Fixture>, pos: Pos2, modifiers: Modifiers) {
    harness.event(Event::ModifiersChanged(modifiers));
    harness.event(Event::PointerMoved(pos));
    harness.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers });
    harness.step();
    harness.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers });
    harness.event(Event::ModifiersChanged(Modifiers::NONE));
    harness.run();
}

fn click_end(harness: &mut Harness<'_, Fixture>, row: usize) {
    let pos = harness.state().view.geometry().unwrap().cell_left(row, 40);
    press(harness, pos, Modifiers::NONE);
}

fn type_text(harness: &mut Harness<'_, Fixture>, typed: &str) {
    for character in typed.chars() {
        harness.event(Event::Text(character.to_string()));
        harness.run();
    }
}

fn content(harness: &Harness<'_, Fixture>) -> String {
    harness.state().state.text().to_string()
}

#[test]
fn typing_a_word_lists_matching_words_and_enter_accepts() {
    let mut harness = harness("alphabet alpine beta\n");
    click_end(&mut harness, 1);

    type_text(&mut harness, "alp");
    // Equal matches: the shorter word first.
    assert_eq!(harness.state().view.completion.shown_labels(), vec!["alpine", "alphabet"]);

    harness.key_press(Key::ArrowDown);
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(content(&harness), "alphabet alpine beta\nalphabet");
    assert!(!harness.state().view.completion.is_open());
}

#[test]
fn the_list_narrows_as_the_prefix_grows_and_closes_when_nothing_matches() {
    let mut harness = harness("alphabet alpine\n");
    click_end(&mut harness, 1);

    type_text(&mut harness, "alph");
    assert_eq!(harness.state().view.completion.shown_labels(), vec!["alphabet"]);
    type_text(&mut harness, "z");
    assert!(!harness.state().view.completion.is_open());
}

#[test]
fn ctrl_space_opens_the_list_and_escape_only_closes_it() {
    let mut harness = harness("one two\n");
    click_end(&mut harness, 1);

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Space);
    harness.run();
    assert!(harness.state().view.completion.shown_labels().contains(&"helper".to_string()));

    harness.key_press(Key::Escape);
    harness.run();
    assert!(!harness.state().view.completion.is_open());
    assert_eq!(content(&harness), "one two\n");
}

#[test]
fn trigger_chars_open_the_list() {
    let mut harness = harness("value\n");
    click_end(&mut harness, 1);

    type_text(&mut harness, "value.");

    assert!(harness.state().view.completion.is_open());
}

#[test]
fn accepting_with_tab_inserts_at_every_cursor() {
    let mut harness = harness("helper\nh\nh");
    click_end(&mut harness, 1);
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::ArrowDown);
    harness.run();

    type_text(&mut harness, "e");
    harness.key_press(Key::Tab);
    harness.run();

    assert_eq!(content(&harness), "helper\nhelper\nhelper");
}

#[test]
fn f12_and_ctrl_click_go_to_the_definition() {
    let mut harness = harness("fn helper() {}\nhelper();\n");
    click_end(&mut harness, 1);

    harness.key_press(Key::F12);
    harness.run();
    let head = |harness: &Harness<'_, Fixture>| {
        harness.state().state.text().position(harness.state().state.selections().primary().head)
    };
    assert_eq!(head(&harness), Position::new(0, 3));

    let pos = harness.state().view.geometry().unwrap().cell_center(1, 2);
    press(&mut harness, pos, Modifiers::COMMAND);
    assert_eq!(head(&harness), Position::new(0, 3));
}

#[test]
fn service_hover_text_shows_in_a_tooltip() {
    let mut harness = harness("helper();\n");
    let pos = harness.state().view.geometry().unwrap().cell_center(0, 2);

    harness.event(Event::PointerMoved(pos));
    for _ in 0..60 {
        harness.step();
    }

    assert!(harness.query_by_label("fn helper() -> u8").is_some());
}

#[test]
fn escape_in_the_editor_closes_the_find_widget() {
    let mut harness = harness("abc\n");
    click_end(&mut harness, 0);
    harness.state_mut().view.find.is_open = true;
    harness.run();

    harness.key_press(Key::Escape);
    harness.run();

    assert!(!harness.state().view.find.is_open);
}
