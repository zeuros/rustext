//! Drives the `CodeEditor` widget headlessly with egui_kittest: real egui input events in,
//! editor state out.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rustext_core::{EditorState, Position, Selection};
use rustext_egui::{CodeEditor, EditorGeometry, EditorTheme, EditorView, Keymap, ViewConfig};

struct Fixture {
    decorations: Vec<rustext_core::decoration::Decoration>,
    state: EditorState,
    view: EditorView,
    config: ViewConfig,
    theme: EditorTheme,
    keymap: Keymap,
    copied: Vec<String>,
}

fn harness(content: &str) -> Harness<'static, Fixture> {
    let fixture = Fixture {
        decorations: Vec::new(),
        state: EditorState::new(content),
        view: EditorView::default(),
        config: ViewConfig { cursor_blinking: false, ..ViewConfig::default() },
        theme: EditorTheme::dark(),
        keymap: Keymap::vscode(),
        copied: Vec::new(),
    };
    let mut harness = Harness::builder().with_size(Vec2::new(800.0, 400.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, fixture: &mut Fixture| {
            let response = CodeEditor::new(
                &mut fixture.state,
                &mut fixture.view,
                &fixture.config,
                &fixture.theme,
                &fixture.keymap,
            )
            .decorations(&fixture.decorations)
            .show(ui);
            if let Some(clipboard) = response.outcome.clipboard {
                fixture.copied.push(clipboard.text);
            }
        },
        fixture,
    );
    harness.run();
    harness
}

fn geometry(harness: &Harness<'_, Fixture>) -> EditorGeometry {
    harness.state().view.geometry().expect("painted once")
}

fn press_at(harness: &mut Harness<'_, Fixture>, pos: Pos2, modifiers: Modifiers) {
    harness.event(Event::ModifiersChanged(modifiers));
    harness.event(Event::PointerMoved(pos));
    harness.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers });
    harness.step();
    harness.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers });
    harness.step();
    harness.event(Event::ModifiersChanged(Modifiers::NONE));
}

fn click(harness: &mut Harness<'_, Fixture>, line: usize, column: usize) {
    let pos = geometry(harness).cell_left(line, column);
    press_at(harness, pos, Modifiers::NONE);
}

fn type_text(harness: &mut Harness<'_, Fixture>, typed: &str) {
    harness.event(Event::Text(typed.to_string()));
    harness.run();
}

fn content(harness: &Harness<'_, Fixture>) -> String {
    harness.state().state.text().to_string()
}

fn head(harness: &Harness<'_, Fixture>) -> Position {
    let state = &harness.state().state;
    state.text().position(state.selections().primary().head)
}

#[test]
fn click_places_the_cursor_and_typing_inserts_there() {
    let mut harness = harness("hello\nworld");

    click(&mut harness, 1, 2);
    type_text(&mut harness, "X");

    assert_eq!(content(&harness), "hello\nwoXrld");
}

#[test]
fn click_past_the_line_end_goes_to_the_line_end() {
    let mut harness = harness("ab\nlonger line");

    click(&mut harness, 0, 30);

    assert_eq!(head(&harness), Position::new(0, 2));
}

#[test]
fn keys_run_keymap_commands() {
    let mut harness = harness("one\ntwo");
    click(&mut harness, 0, 0);

    harness.key_press(Key::End);
    harness.key_press_modifiers(Modifiers::SHIFT, Key::ArrowLeft);
    harness.run();
    type_text(&mut harness, "E");
    harness.key_press_modifiers(Modifiers::ALT, Key::ArrowDown);
    harness.run();

    assert_eq!(content(&harness), "two\nonE");
    assert_eq!(head(&harness), Position::new(1, 3));
}

#[test]
fn chords_wait_for_their_second_key() {
    let mut harness = harness("abc");
    click(&mut harness, 0, 1);

    harness.key_press_modifiers(Modifiers::COMMAND, Key::K);
    harness.run();
    assert_eq!(harness.state().view.pending_chords().len(), 1);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::U);
    harness.run();

    assert_eq!(content(&harness), "ABC");
    assert!(harness.state().view.pending_chords().is_empty());
}

#[test]
fn double_click_selects_a_word() {
    let mut harness = harness("let value = 1;");
    let pos = geometry(&harness).cell_center(0, 6);

    press_at(&mut harness, pos, Modifiers::NONE);
    press_at(&mut harness, pos, Modifiers::NONE);

    assert_eq!(harness.state().state.selections().primary(), Selection::new(4, 9));
}

#[test]
fn drag_selects_text() {
    let mut harness = harness("abcdef\nghijkl");
    let geometry = geometry(&harness);
    let (from, to) = (geometry.cell_left(0, 1), geometry.cell_left(1, 3));

    harness.event(Event::PointerMoved(from));
    harness.event(Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.step();
    harness.event(Event::PointerMoved(to));
    harness.step();
    harness.event(Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run();

    assert_eq!(harness.state().state.selections().primary(), Selection::new(1, 10));
}

#[test]
fn alt_click_adds_a_cursor() {
    let mut harness = harness("aaa\nbbb");
    click(&mut harness, 0, 1);
    let pos = geometry(&harness).cell_left(1, 2);

    press_at(&mut harness, pos, Modifiers::ALT);
    type_text(&mut harness, "_");

    assert_eq!(content(&harness), "a_aa\nbb_b");
}

#[test]
fn copy_event_copies_the_selection() {
    let mut harness = harness("copy me");
    click(&mut harness, 0, 0);
    harness.key_press_modifiers(Modifiers::SHIFT, Key::End);
    harness.run();

    harness.event(Event::Copy);
    harness.run();

    assert_eq!(harness.state().copied, vec!["copy me".to_string()]);
}

#[test]
fn paste_event_inserts_text() {
    let mut harness = harness("ab");
    click(&mut harness, 0, 1);

    harness.event(Event::Paste("XY".into()));
    harness.run();

    assert_eq!(content(&harness), "aXYb");
}

#[test]
fn gutter_click_selects_the_line() {
    let mut harness = harness("one\ntwo\nthree");
    let geometry = geometry(&harness);
    let pos = Pos2::new(geometry.text_rect.left() - 10.0, geometry.row_top(1) + geometry.row_height / 2.0);

    press_at(&mut harness, pos, Modifiers::NONE);

    assert_eq!(harness.state().state.selections().primary(), Selection::new(4, 8));
}

#[test]
fn enter_and_tab_keys_edit_instead_of_moving_focus() {
    let mut harness = harness("x");
    click(&mut harness, 0, 1);

    harness.key_press(Key::Enter);
    harness.key_press(Key::Tab);
    harness.run();
    type_text(&mut harness, "y");

    assert_eq!(content(&harness), "x\n    y");
}

fn selected_text(harness: &Harness<'_, Fixture>) -> String {
    let state = &harness.state().state;
    let primary = state.selections().primary();
    state.text().slice(primary.start(), primary.end())
}

#[test]
fn ctrl_f_seeds_the_word_and_enter_walks_the_matches() {
    let mut harness = harness("alpha beta\nalpha gamma\nalpha");
    click(&mut harness, 0, 1);

    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    harness.run();
    assert!(harness.state().view.find.is_open);
    assert_eq!(harness.state().view.find.query.pattern, "alpha");
    assert_eq!(harness.state().view.find.matches().len(), 3);

    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(head(&harness), Position::new(1, 5));
    assert_eq!(selected_text(&harness), "alpha");

    harness.key_press_modifiers(Modifiers::SHIFT, Key::Enter);
    harness.run();
    assert_eq!(head(&harness), Position::new(0, 5));
}

#[test]
fn typing_a_query_selects_the_first_match_after_the_cursor() {
    let mut harness = harness("one two three two");
    click(&mut harness, 0, 5);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    harness.run();

    harness.event(Event::Key {
        key: Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });
    type_text(&mut harness, "two");

    assert_eq!(harness.state().state.selections().primary(), Selection::new(14, 17));
}

#[test]
fn replace_all_from_the_find_widget_state() {
    let mut harness = harness("x = 1; y = x + x;");
    click(&mut harness, 0, 0);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::H);
    harness.run();
    harness.state_mut().view.find.replacement = "value".into();
    harness.state_mut().view.find.query.whole_word = true;
    harness.run();

    harness.get_by_label("Replace all").click();
    harness.run();

    assert_eq!(content(&harness), "value = 1; y = value + value;");
}

#[test]
fn escape_closes_the_find_widget() {
    let mut harness = harness("abc");
    click(&mut harness, 0, 1);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    harness.run();

    harness.key_press(Key::Escape);
    harness.run();

    assert!(!harness.state().view.find.is_open);
}

#[test]
fn ctrl_g_goes_to_a_line_and_column() {
    let mut harness = harness("a\nb\nccccc\nd");
    click(&mut harness, 0, 0);

    harness.key_press_modifiers(Modifiers::COMMAND, Key::G);
    harness.run();
    type_text(&mut harness, "3:4");
    harness.key_press(Key::Enter);
    harness.run();

    assert_eq!(head(&harness), Position::new(2, 3));
    assert!(!harness.state().view.go_to_line.is_open);
}

const FOLDABLE: &str = "fn a() {\n    one();\n    two();\n}\nfn b() {\n    three();\n}\n";

fn chord(harness: &mut Harness<'_, Fixture>, second_key: Key) {
    harness.key_press_modifiers(Modifiers::COMMAND, Key::K);
    harness.key_press_modifiers(Modifiers::COMMAND, second_key);
    harness.run();
}

#[test]
fn fold_all_hides_blocks_and_the_cursor_steps_over_them() {
    let mut harness = harness(FOLDABLE);
    click(&mut harness, 0, 0);

    chord(&mut harness, Key::Num0);
    assert_eq!(harness.state().view.folds.folded_header_count(), 2);

    harness.key_press(Key::ArrowDown);
    harness.run();
    assert_eq!(head(&harness).line, 3);
    harness.key_press(Key::ArrowDown);
    harness.run();
    assert_eq!(head(&harness).line, 4);

    chord(&mut harness, Key::J);
    assert_eq!(harness.state().view.folds.folded_header_count(), 0);
}

#[test]
fn moving_the_cursor_into_a_fold_unfolds_it() {
    let mut harness = harness(FOLDABLE);
    click(&mut harness, 0, 0);
    chord(&mut harness, Key::Num0);

    harness.state_mut().state.execute(rustext_core::Command::SetSelections(rustext_core::Selections::cursor(15)));
    harness.run();

    assert_eq!(harness.state().view.folds.folded_header_count(), 1);
}

#[test]
fn clicking_the_fold_marker_toggles_the_fold() {
    let mut harness = harness(FOLDABLE);
    let geometry = geometry(&harness);
    let marker = Pos2::new(
        geometry.text_rect.left() - geometry.char_width * 1.9,
        geometry.row_top(0) + geometry.row_height / 2.0,
    );

    press_at(&mut harness, marker, Modifiers::NONE);
    assert_eq!(harness.state().view.folds.folded_header_count(), 1);

    // Folded: line 3 ("}") now sits on row 1.
    click(&mut harness, 1, 0);
    assert_eq!(head(&harness).line, 3);
}

#[test]
fn ctrl_shift_backslash_jumps_to_the_matching_bracket() {
    let mut harness = harness("f(a, (b))");
    click(&mut harness, 0, 1);

    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Backslash);
    harness.run();
    assert_eq!(head(&harness), Position::new(0, 8));

    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Backslash);
    harness.run();
    assert_eq!(head(&harness), Position::new(0, 1));
}

#[test]
fn jump_to_bracket_skips_brackets_in_strings_and_comments() {
    let mut harness = harness("f(\"(\", // (\n x)");
    harness.state_mut().state.language = rustext_core::LanguageConfiguration::c_like();
    click(&mut harness, 0, 1);

    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Backslash);
    harness.run();
    assert_eq!(head(&harness), Position::new(1, 2));
}

#[test]
fn f8_walks_the_diagnostics() {
    let mut harness = harness("let a = 1;\nlet b = 2;\nlet c = 3;");
    let theme = EditorTheme::dark();
    harness.state_mut().decorations = vec![
        rustext_egui::diagnostic(15, 16, rustext_egui::Severity::Error, "unused b", &theme),
        rustext_egui::diagnostic(26, 27, rustext_egui::Severity::Warning, "unused c", &theme),
    ];
    click(&mut harness, 0, 0);

    harness.key_press(Key::F8);
    harness.run();
    assert_eq!(head(&harness), Position::new(1, 4));
    harness.key_press(Key::F8);
    harness.run();
    assert_eq!(head(&harness), Position::new(2, 4));
    harness.key_press(Key::F8);
    harness.run();
    assert_eq!(head(&harness), Position::new(1, 4));
}

#[test]
fn hovering_a_diagnostic_shows_its_message() {
    let mut harness = harness("let a = 1;");
    let theme = EditorTheme::dark();
    harness.state_mut().decorations =
        vec![rustext_egui::diagnostic(4, 5, rustext_egui::Severity::Error, "unused variable", &theme)];
    harness.run();
    let pos = geometry(&harness).cell_center(0, 4);

    harness.event(Event::PointerMoved(pos));
    for _ in 0..60 {
        harness.step();
    }

    assert!(harness.query_by_label("unused variable").is_some());
}

#[test]
fn hovering_a_lookalike_letter_says_what_it_looks_like() {
    let mut harness = harness("let p\u{0430}ypal = 1;");
    let pos = geometry(&harness).cell_center(0, 5);

    harness.event(Event::PointerMoved(pos));
    for _ in 0..60 {
        harness.step();
    }

    assert!(harness.query_by_label("U+0430 'а' looks like ASCII 'a'").is_some());
}

#[test]
fn folds_below_an_edit_shift_with_it() {
    let mut harness = harness(FOLDABLE);
    click(&mut harness, 0, 0);
    chord(&mut harness, Key::Num0);
    assert_eq!(harness.state().view.folds.folded_header_lines(), vec![0, 4]);

    harness.key_press(Key::Home);
    harness.key_press(Key::Enter);
    harness.key_press(Key::Enter);
    harness.run();

    assert_eq!(harness.state().view.folds.folded_header_lines(), vec![2, 6]);
}

fn wrapping_harness(content: &str, columns: usize) -> Harness<'static, Fixture> {
    let mut harness = harness(content);
    harness.state_mut().config.word_wrap = rustext_egui::code_editor::WordWrap::Column(columns);
    harness.run();
    harness
}

#[test]
fn long_lines_wrap_into_continuation_rows() {
    // "alpha beta gamma delta" wraps at 12 columns after "alpha beta " and "gamma delta".
    let mut harness = wrapping_harness("alpha beta gamma delta\nend", 12);

    // Row 1 is the continuation "gamma delta"; its column 2 is the line's column 13.
    click(&mut harness, 1, 2);
    assert_eq!(head(&harness), Position::new(0, 13));

    // Row 2 is line 1.
    click(&mut harness, 2, 1);
    assert_eq!(head(&harness), Position::new(1, 1));
}

#[test]
fn editing_rewraps_the_edited_line() {
    let mut harness = wrapping_harness("short\nend", 12);
    click(&mut harness, 0, 5);

    type_text(&mut harness, " and now much longer");

    // The line wraps: "end" moved down from row 1.
    click(&mut harness, 3, 1);
    assert_eq!(head(&harness), Position::new(1, 1));
}

#[test]
fn dragging_the_horizontal_scrollbar_scrolls_long_lines() {
    let mut harness = harness(&format!("{}\nshort", "x".repeat(400)));
    let geometry = geometry(&harness);
    let track_y = geometry.text_rect.bottom() - 5.0;
    let from = Pos2::new(geometry.text_rect.left() + 5.0, track_y);
    let to = Pos2::new(geometry.text_rect.right() - 5.0, track_y);

    harness.event(Event::PointerMoved(from));
    harness.event(Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.step();
    harness.event(Event::PointerMoved(to));
    harness.step();
    harness.event(Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run();

    assert!(harness.state().view.scroll_offset.x > 1000.0, "scrolled to {}", harness.state().view.scroll_offset.x);
    assert_eq!(head(&harness), Position::new(0, 0), "the drag must not move the cursor");
}

#[test]
fn sticky_scroll_pins_the_enclosing_header_and_a_click_goes_back_to_it() {
    let body: String = (0..60).map(|index| format!("    line({index});\n")).collect();
    let mut harness = harness(&format!("fn long() {{\n{body}}}\nfn next() {{}}\n"));
    let row_height = geometry(&harness).row_height;

    harness.state_mut().view.scroll_offset.y = 20.0 * row_height;
    harness.run();
    assert_eq!(harness.state().view.sticky_lines(), &[0]);

    let geometry = geometry(&harness);
    let pinned_row = Pos2::new(geometry.text_rect.left() + 20.0, geometry.text_rect.top() + row_height / 2.0);
    press_at(&mut harness, pinned_row, Modifiers::NONE);

    assert_eq!(head(&harness), Position::new(0, 0));
    assert!(harness.state().view.sticky_lines().is_empty());
    assert!(harness.state().view.scroll_offset.y < row_height);
}

#[test]
fn the_command_palette_runs_a_command_by_name() {
    let mut harness = harness("b\na\nc");
    click(&mut harness, 0, 0);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();

    harness.key_press(Key::F1);
    harness.run();
    assert!(harness.state().view.palette.is_open);
    harness.event(Event::Text("sort asc".into()));
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();

    assert_eq!(content(&harness), "a\nb\nc");
    assert!(!harness.state().view.palette.is_open);
}

#[test]
fn up_and_down_move_by_visual_row_when_wrapping() {
    // Rows: "alpha beta " / "gamma delta" / "end".
    let mut harness = wrapping_harness("alpha beta gamma delta\nend", 12);
    click(&mut harness, 0, 2);

    harness.key_press(Key::ArrowDown);
    harness.run();
    assert_eq!(head(&harness), Position::new(0, 13));
    harness.key_press(Key::ArrowDown);
    harness.run();
    assert_eq!(head(&harness), Position::new(1, 2));
    harness.key_press(Key::ArrowUp);
    harness.key_press(Key::ArrowUp);
    harness.run();
    assert_eq!(head(&harness), Position::new(0, 2));
}

#[test]
fn injected_text_shifts_what_follows_but_is_not_text() {
    use rustext_core::decoration::Decoration;
    use rustext_core::style::Rgba;
    // "f(ab)" shown as "f(x: ab)": the hint takes three cells before column 2.
    let mut harness = harness("f(ab)");
    harness.state_mut().decorations = vec![Decoration::injected_text(2, "x: ", Rgba::hex(0x888888))];
    harness.run();

    click(&mut harness, 0, 6);
    assert_eq!(head(&harness), Position::new(0, 3));

    type_text(&mut harness, "Z");
    assert_eq!(content(&harness), "f(aZb)");
}

#[test]
fn find_over_a_multi_line_selection_searches_only_in_it() {
    let mut harness = harness("a x\nb x\nc x");
    click(&mut harness, 0, 0);
    harness.key_press_modifiers(Modifiers::SHIFT, Key::ArrowDown);
    harness.key_press_modifiers(Modifiers::SHIFT, Key::End);
    harness.run();

    harness.key_press_modifiers(Modifiers::COMMAND, Key::H);
    harness.run();
    harness.state_mut().view.find.query.pattern = "x".into();
    harness.state_mut().view.find.replacement = "y".into();
    harness.run();
    assert_eq!(harness.state().view.find.matches().len(), 2);

    harness.get_by_label("Replace all").click();
    harness.run();
    assert_eq!(content(&harness), "a y\nb y\nc x");
}

#[test]
fn screen_readers_see_a_named_text_input_with_the_cursor_line() {
    let mut harness = harness("first line\nsecond line");
    click(&mut harness, 1, 3);

    let node = harness.get_by_label("Editor");
    assert_eq!(node.value().as_deref(), Some("second line"));
}

#[test]
fn screen_readers_read_the_visible_lines_and_the_selection() {
    let mut harness = harness("first line\nsecond line");
    click(&mut harness, 1, 3);

    use egui_kittest::kittest::NodeT;
    let editor = harness.get_by_label("Editor").accesskit_node();
    assert_eq!(editor.role(), egui::Role::MultilineTextInput);
    let runs: Vec<String> = harness.get_all_by_role(egui::Role::TextRun).filter_map(|run| run.value()).collect();
    assert_eq!(runs, vec!["first line\n", "second line"]);
    let selection = editor.data().text_selection().copied();
    assert_eq!(selection.map(|selection| selection.focus.character_index), Some(3));
}
