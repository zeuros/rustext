//! Drives completion, hover and go-to-definition through a test `LanguageServices`.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rustext_core::services::{
    CodeAction, CodeLens, CodeLensAction, CompletionItem, CompletionKind, LanguageServices, Location, Reply,
    ServiceCommand, TextEdit, WordCompletions,
};
use rustext_core::{EditorState, Position, Text};
use rustext_egui::{CodeEditor, EditorTheme, EditorView, Keymap, ViewConfig};

/// Word completions, plus a hover text and a definition for `helper`.
struct TestServices {
    words: WordCompletions,
    /// Published once, on the next frame.
    pending_diagnostics: Option<Vec<rustext_core::services::Diagnostic>>,
    /// The line ranges inlay hints were asked for.
    inlay_hint_requests: Vec<std::ops::Range<usize>>,
    /// The code lenses to answer, and how many times they were asked for.
    code_lenses: Vec<CodeLens>,
    code_lens_requests: usize,
}

impl LanguageServices for TestServices {
    fn completions(&mut self, text: &Text, revision: u64, position: Position) -> Reply<Vec<CompletionItem>> {
        let Reply::Ready(mut items) = self.words.completions(text, revision, position) else { return Reply::Pending };
        items.push(CompletionItem::new("helper", CompletionKind::Function).with_detail("fn() -> u8"));
        items.push(CompletionItem::snippet("forloop", "for ${1:item} in ${2:items} {\n    $0\n}"));
        Reply::Ready(items)
    }

    fn inline_completion(&mut self, text: &Text, _revision: u64, position: Position) -> Reply<Option<String>> {
        let before: String = text.line_content(position.line).chars().take(position.column).collect();
        Reply::Ready(before.ends_with("zz").then(|| "_top".to_string()))
    }

    fn rename(
        &mut self,
        text: &Text,
        _revision: u64,
        position: Position,
        new_name: &str,
    ) -> Reply<Vec<rustext_core::services::TextEdit>> {
        use rustext_core::find::{SearchQuery, find_matches};
        let line: Vec<char> = text.line_content(position.line).chars().collect();
        let Some((start, end)) = WordCompletions::default().words.word_at(&line, position.column) else {
            return Reply::Ready(Vec::new());
        };
        let word: String = line[start..end].iter().collect();
        let query = SearchQuery { match_case: true, whole_word: true, ..SearchQuery::new(word) };
        let edits = find_matches(text, &query, None, 100)
            .unwrap()
            .into_iter()
            .map(|found| rustext_core::services::TextEdit {
                range: rustext_core::Range::new(text.position(found.start), text.position(found.end)),
                new_text: new_name.to_string(),
            })
            .collect();
        Reply::Ready(edits)
    }

    fn signature_help(
        &mut self,
        text: &Text,
        _revision: u64,
        position: Position,
    ) -> Reply<Option<rustext_core::services::SignatureHelp>> {
        let before: String = text.line_content(position.line).chars().take(position.column).collect();
        let Some(open) = before.rfind("helper(") else { return Reply::Ready(None) };
        let commas = before[open..].matches(',').count();
        let label = "helper(first: u8, second: u8)".to_string();
        let active_parameter = [(7, 16), (18, 28)].get(commas).copied();
        Reply::Ready(Some(rustext_core::services::SignatureHelp { label, active_parameter, documentation: None }))
    }

    /// On `helper`, its calls (`helper(`) only, unlike the word highlight; `None` elsewhere.
    fn document_highlights(
        &mut self,
        text: &Text,
        _revision: u64,
        position: Position,
    ) -> Reply<Option<Vec<rustext_core::Range>>> {
        let line: Vec<char> = text.line_content(position.line).chars().collect();
        let on_helper = WordCompletions::default()
            .words
            .word_at(&line, position.column)
            .is_some_and(|(start, end)| line[start..end].iter().collect::<String>() == "helper");
        if !on_helper {
            return Reply::Ready(None);
        }
        let calls = (0..text.line_count())
            .flat_map(|line| {
                let content = text.line_content(line);
                content
                    .match_indices("helper(")
                    .map(|(byte, _)| content[..byte].chars().count())
                    .map(move |column| {
                        rustext_core::Range::new(Position::new(line, column), Position::new(line, column + 6))
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        Reply::Ready(Some(calls))
    }

    /// Every whole-word `helper`, and one in another document.
    fn references(&mut self, text: &Text, _revision: u64, _position: Position) -> Reply<Vec<Location>> {
        use rustext_core::find::{SearchQuery, find_matches};
        let query = SearchQuery { match_case: true, whole_word: true, ..SearchQuery::new("helper") };
        let found = find_matches(text, &query, None, 100).unwrap();
        Reply::Ready(
            found
                .into_iter()
                .map(|found| {
                    Location::here(rustext_core::Range::new(text.position(found.start), text.position(found.end)))
                })
                .chain([elsewhere()])
                .collect(),
        )
    }

    /// `: i32` after the name of every `let`.
    fn inlay_hints(
        &mut self,
        text: &Text,
        _revision: u64,
        lines: std::ops::Range<usize>,
    ) -> Reply<Vec<rustext_core::services::InlayHint>> {
        self.inlay_hint_requests.push(lines.clone());
        let hints = lines
            .filter_map(|line| {
                let content = text.line_content(line);
                let name = content.strip_prefix("let ")?.split([' ', '=']).next()?;
                let position = Position::new(line, 4 + name.chars().count());
                Some(rustext_core::services::InlayHint { position, label: ": i32".into() })
            })
            .collect();
        Reply::Ready(hints)
    }

    fn take_diagnostics(&mut self, _text: &Text) -> Option<Vec<rustext_core::services::Diagnostic>> {
        self.pending_diagnostics.take()
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

    /// A refactoring, then a preferred fix that ends the cursor's line with `;`.
    fn code_actions(&mut self, text: &Text, _revision: u64, range: rustext_core::Range) -> Reply<Vec<CodeAction>> {
        let line_end = Position::new(range.start.line, text.line_len(range.start.line));
        let at_line_end = rustext_core::Range::new(line_end, line_end);
        let edit = |new_text: &str| vec![TextEdit { range: at_line_end, new_text: new_text.into() }];
        Reply::Ready(vec![
            CodeAction { title: "Add semicolon".into(), is_quick_fix: true, is_preferred: true, edits: edit(";") },
            CodeAction {
                title: "Add comment".into(),
                is_quick_fix: false,
                is_preferred: false,
                edits: edit(" // note"),
            },
        ])
    }

    /// Puts a header line above the range's first line.
    fn format_range(
        &mut self,
        _text: &Text,
        _revision: u64,
        range: rustext_core::Range,
        _indentation: rustext_core::indentation::Indentation,
    ) -> Reply<Vec<TextEdit>> {
        let start = Position::new(range.start.line, 0);
        Reply::Ready(vec![TextEdit { range: rustext_core::Range::new(start, start), new_text: "// range\n".into() }])
    }

    /// Puts a header line on top, saying the tab size.
    fn format_document(
        &mut self,
        _text: &Text,
        _revision: u64,
        indentation: rustext_core::indentation::Indentation,
    ) -> Reply<Vec<TextEdit>> {
        let start = Position::new(0, 0);
        let header = format!("// tab size {}\n", indentation.tab_size);
        Reply::Ready(vec![TextEdit { range: rustext_core::Range::new(start, start), new_text: header }])
    }

    fn code_lenses(&mut self, _text: &Text, _revision: u64) -> Reply<Vec<CodeLens>> {
        self.code_lens_requests += 1;
        Reply::Ready(self.code_lenses.clone())
    }

    fn format_on_type_trigger_chars(&self) -> &[char] {
        &[';', '\n']
    }

    /// Indents the cursor's line by four spaces.
    fn format_on_type(
        &mut self,
        text: &Text,
        _revision: u64,
        position: Position,
        _typed: char,
        _indentation: rustext_core::indentation::Indentation,
    ) -> Reply<Vec<TextEdit>> {
        let content = text.line_content(position.line);
        let indent_len = content.chars().take_while(|character| character.is_whitespace()).count();
        let range = rustext_core::Range::new(Position::new(position.line, 0), Position::new(position.line, indent_len));
        Reply::Ready(vec![TextEdit { range, new_text: "    ".into() }])
    }

    /// `helper` is defined at 0:3; `imported` in another document.
    fn definition(&mut self, text: &Text, _revision: u64, position: Position) -> Reply<Option<Location>> {
        let line: Vec<char> = text.line_content(position.line).chars().collect();
        let word = self
            .words
            .words
            .word_at(&line, position.column)
            .map(|(start, end)| line[start..end].iter().collect::<String>());
        Reply::Ready(match word.as_deref() {
            Some("imported") => Some(elsewhere()),
            _ => Some(Location::here(rustext_core::Range::new(Position::new(0, 3), Position::new(0, 9)))),
        })
    }
}

/// A place in another document.
fn elsewhere() -> Location {
    Location::in_document("file:///other/lib.rs", rustext_core::Range::new(Position::new(41, 4), Position::new(41, 10)))
}

struct Fixture {
    open_requests: Vec<Location>,
    service_commands: Vec<ServiceCommand>,
    state: EditorState,
    view: EditorView,
    config: ViewConfig,
    theme: EditorTheme,
    keymap: Keymap,
    services: TestServices,
}

fn harness(content: &str) -> Harness<'static, Fixture> {
    let fixture = Fixture {
        open_requests: Vec::new(),
        service_commands: Vec::new(),
        state: EditorState::new(content),
        view: EditorView::default(),
        config: ViewConfig { cursor_blinking: false, ..ViewConfig::default() },
        theme: EditorTheme::dark(),
        keymap: Keymap::vscode(),
        services: TestServices {
            words: WordCompletions::default(),
            pending_diagnostics: None,
            inlay_hint_requests: Vec::new(),
            code_lenses: Vec::new(),
            code_lens_requests: 0,
        },
    };
    let mut harness = Harness::builder().with_size(Vec2::new(900.0, 500.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, fixture: &mut Fixture| {
            let response = CodeEditor::new(
                &mut fixture.state,
                &mut fixture.view,
                &fixture.config,
                &fixture.theme,
                &fixture.keymap,
            )
            .services(&mut fixture.services)
            .show(ui);
            fixture.open_requests.extend(response.open_request);
            fixture.service_commands.extend(response.service_command);
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

#[test]
fn snippet_completions_visit_their_tab_stops_with_tab() {
    let mut harness = harness("\n");
    click_end(&mut harness, 0);

    type_text(&mut harness, "forl");
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(content(&harness), "for item in items {\n    \n}\n");

    type_text(&mut harness, "row");
    harness.key_press(Key::Tab);
    harness.run();
    type_text(&mut harness, "rows");
    harness.key_press(Key::Tab);
    harness.run();
    type_text(&mut harness, "x");

    assert_eq!(content(&harness), "for row in rows {\n    x\n}\n");
}

#[test]
fn service_diagnostics_show_follow_edits_and_take_f8() {
    use rustext_core::Range;
    use rustext_core::services::{Diagnostic, Severity};
    let mut harness = harness("let x = 1;\nlet y = 2;\n");
    harness.state_mut().services.pending_diagnostics = Some(vec![Diagnostic {
        range: Range::new(Position::new(1, 4), Position::new(1, 5)),
        severity: Severity::Warning,
        message: "unused y".into(),
    }]);
    click_end(&mut harness, 0);

    // An edit above the diagnostic moves it down a line.
    harness.key_press(Key::Home);
    harness.key_press(Key::Enter);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Home);
    harness.key_press(Key::F8);
    harness.run();

    let head = harness.state().state.text().position(harness.state().state.selections().primary().head);
    assert_eq!(head, Position::new(2, 4));
}

#[test]
fn ghost_text_is_accepted_with_tab_and_dismissed_with_escape() {
    let mut harness = harness("\n");
    click_end(&mut harness, 0);

    type_text(&mut harness, "zz");
    harness.key_press(Key::Tab);
    harness.run();
    assert_eq!(content(&harness), "zz_top\n");

    // "zz" now also lists the word "zz_top": the first Escape closes the list, the second the
    // ghost text; Tab is a Tab again (column 9 to 12).
    type_text(&mut harness, " zz");
    harness.key_press(Key::Escape);
    harness.key_press(Key::Escape);
    harness.key_press(Key::Tab);
    harness.run();
    assert_eq!(content(&harness), "zz_top zz   \n");
}

#[test]
fn f2_renames_every_occurrence_in_one_undo_step() {
    let mut harness = harness("let count = 1;\ncount += count;\n");
    let pos = harness.state().view.geometry().unwrap().cell_center(0, 5);
    press(&mut harness, pos, Modifiers::NONE);

    harness.key_press(Key::F2);
    harness.run();
    assert_eq!(harness.state().view.rename.new_name, "count");
    harness.state_mut().view.rename.new_name = "total".into();
    harness.key_press(Key::Enter);
    harness.run();
    harness.run();
    assert_eq!(content(&harness), "let total = 1;\ntotal += total;\n");

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(content(&harness), "let count = 1;\ncount += count;\n");
}

#[test]
fn signature_help_follows_the_active_parameter() {
    let mut harness = harness("\n");
    click_end(&mut harness, 0);

    type_text(&mut harness, "helper(");
    let shown = |harness: &Harness<'_, Fixture>| harness.state().view.signature.shown_label().map(str::to_string);
    assert_eq!(shown(&harness).as_deref(), Some("helper(first: u8, second: u8)"));
    type_text(&mut harness, "1, 2)");
    assert_eq!(shown(&harness), None);
}

#[test]
fn inlay_hints_show_for_the_visible_block_and_follow_edits() {
    let content: String =
        (0..300).map(|line| if line == 2 { "let a = 1;\n".to_string() } else { format!("// {line}\n") }).collect();
    let mut harness = harness(&content);

    assert_eq!(harness.state().view.inlay_hints.labels(), vec![": i32"]);
    assert_eq!(harness.state().services.inlay_hint_requests.last(), Some(&(0..100)));
    let hint_offset = |harness: &Harness<'_, Fixture>| harness.state().view.inlay_hints.decorations()[0].start;
    let before = hint_offset(&harness);

    let pos = harness.state().view.geometry().unwrap().cell_left(0, 0);
    press(&mut harness, pos, Modifiers::NONE);
    type_text(&mut harness, "x");
    assert_eq!(hint_offset(&harness), before + 1);
}

#[test]
fn small_scrolls_reuse_the_inlay_hints_answer() {
    let content = "let a = 1;\n".repeat(300);
    let mut harness = harness(&content);
    let requests_before = harness.state().services.inlay_hint_requests.len();

    harness.state_mut().view.scroll_offset.y = 200.0;
    harness.run();
    assert_eq!(harness.state().services.inlay_hint_requests.len(), requests_before);

    harness.state_mut().view.scroll_offset.y = 3_000.0;
    harness.run();
    assert_eq!(harness.state().services.inlay_hint_requests.last().map(|lines| lines.start), Some(100));
}

#[test]
fn document_highlights_from_the_services_replace_the_word_highlight() {
    let mut harness = harness("helper();\nlet helper = 1;\nhelper();\nplain plain\n");
    let pos = harness.state().view.geometry().unwrap().cell_center(0, 2);
    press(&mut harness, pos, Modifiers::NONE);

    let highlights = |harness: &Harness<'_, Fixture>| {
        let fixture = harness.state();
        let head = fixture.state.selections().primary().head;
        fixture.view.document_highlights.ranges_for(fixture.state.text_revision(), head).map(<[_]>::to_vec)
    };
    assert_eq!(highlights(&harness), Some(vec![(0, 6), (26, 32)]));

    let pos = harness.state().view.geometry().unwrap().cell_center(3, 2);
    press(&mut harness, pos, Modifiers::NONE);
    assert_eq!(highlights(&harness), None);
}

#[test]
fn shift_f12_lists_references_and_enter_jumps_to_one() {
    let mut harness = harness("fn helper() {}\nhelper();\nlet x = helper();\n");
    let pos = harness.state().view.geometry().unwrap().cell_center(1, 2);
    press(&mut harness, pos, Modifiers::NONE);

    harness.key_press_modifiers(Modifiers::SHIFT, Key::F12);
    harness.run();
    let references = &harness.state().view.references;
    let text = harness.state().state.text();
    let here: Vec<(usize, usize)> = references
        .entries()
        .iter()
        .filter(|location| location.document.is_none())
        .map(|location| text.range_offsets(location.range))
        .collect();
    assert_eq!(here, vec![(3, 9), (15, 21), (33, 39)]);
    assert_eq!(references.entries().last(), Some(&elsewhere()));
    assert_eq!(references.selected(), 1);
    harness.get_by_label("4 references");

    harness.key_press(Key::ArrowDown);
    harness.key_press(Key::Enter);
    harness.run();
    assert!(!harness.state().view.references.is_open());
    let primary = harness.state().state.selections().primary();
    assert_eq!((primary.anchor, primary.head), (33, 39));
}

#[test]
fn a_reference_in_another_document_asks_the_application_to_open_it() {
    let mut harness = harness("helper();\n");
    let pos = harness.state().view.geometry().unwrap().cell_center(0, 2);
    press(&mut harness, pos, Modifiers::NONE);

    harness.key_press_modifiers(Modifiers::SHIFT, Key::F12);
    harness.run();
    harness.key_press(Key::ArrowDown);
    harness.key_press(Key::Enter);
    harness.run();

    assert_eq!(harness.state().open_requests, vec![elsewhere()]);
    assert!(!harness.state().view.references.is_open());
}

#[test]
fn a_definition_in_another_document_asks_the_application_to_open_it() {
    let mut harness = harness("imported();\n");
    let pos = harness.state().view.geometry().unwrap().cell_center(0, 2);
    press(&mut harness, pos, Modifiers::NONE);

    harness.key_press(Key::F12);
    harness.run();

    assert_eq!(harness.state().open_requests, vec![elsewhere()]);
}

#[test]
fn shift_alt_f_formats_keeping_the_cursor_on_its_text_and_undoes_in_one_step() {
    let mut harness = harness("let x = 1;\n");
    let pos = harness.state().view.geometry().unwrap().cell_center(0, 4);
    press(&mut harness, pos, Modifiers::NONE);
    let head_before = harness.state().state.selections().primary().head;

    harness.key_press_modifiers(Modifiers::SHIFT | Modifiers::ALT, Key::F);
    harness.run();
    assert_eq!(harness.state().state.text().to_string(), "// tab size 4\nlet x = 1;\n");
    let head = harness.state().state.selections().primary().head;
    assert_eq!(head, head_before + "// tab size 4\n".chars().count());

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(harness.state().state.text().to_string(), "let x = 1;\n");
}

#[test]
fn ctrl_period_lists_code_actions_and_enter_applies_one() {
    let mut harness = harness("let x = 1\n");
    let pos = harness.state().view.geometry().unwrap().cell_center(0, 4);
    press(&mut harness, pos, Modifiers::NONE);

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Period);
    harness.run();
    harness.get_by_label("Add semicolon");
    harness.key_press(Key::Enter);
    harness.run();

    assert_eq!(harness.state().state.text().to_string(), "let x = 1;\n");
    assert!(!harness.state().view.code_actions.is_open());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(harness.state().state.text().to_string(), "let x = 1\n");
}

#[test]
fn a_lightbulb_shows_where_the_cursor_rests_on_code_actions_and_a_click_lists_them() {
    let mut harness = harness("let x = 1\n");
    let pos = harness.state().view.geometry().unwrap().cell_center(0, 4);
    press(&mut harness, pos, Modifiers::NONE);
    assert!(!harness.state().view.shows_lightbulb());

    for _ in 0..30 {
        harness.step();
    }
    assert!(harness.state().view.shows_lightbulb());

    let bulb = harness.state().view.lightbulb_rect().unwrap().center();
    press(&mut harness, bulb, Modifiers::NONE);
    assert!(harness.state().view.code_actions.is_open());
}

#[test]
fn ctrl_k_ctrl_f_formats_the_selected_lines() {
    let mut harness = harness("a\nb\n");
    let pos = harness.state().view.geometry().unwrap().cell_center(1, 0);
    press(&mut harness, pos, Modifiers::NONE);

    harness.key_press_modifiers(Modifiers::COMMAND, Key::K);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    harness.run();

    assert_eq!(harness.state().state.text().to_string(), "a\n// range\nb\n");
}

#[test]
fn typing_a_trigger_char_formats_around_the_cursor_and_undoes_apart() {
    let mut harness = harness("fn f() {\nx\n}");
    harness.state_mut().config.format_on_type = true;
    click_end(&mut harness, 1);

    type_text(&mut harness, ";");

    assert_eq!(content(&harness), "fn f() {\n    x;\n}");
    assert_eq!(harness.state().state.selections().primary().head, "fn f() {\n    x;".chars().count());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(content(&harness), "fn f() {\nx;\n}");
}

#[test]
fn enter_formats_the_new_line_on_type() {
    // Without formatting, Enter after an unindented line opens an unindented line.
    let mut harness = harness("x\ny");
    harness.state_mut().config.format_on_type = true;
    click_end(&mut harness, 0);

    harness.key_press(Key::Enter);
    harness.run();

    assert_eq!(content(&harness), "x\n    \ny");
}

#[test]
fn formatting_on_type_is_off_by_default() {
    let mut harness = harness("fn f() {\nx\n}");
    click_end(&mut harness, 1);

    type_text(&mut harness, ";");

    assert_eq!(content(&harness), "fn f() {\nx;\n}");
}

#[test]
fn pasted_text_is_formatted_when_format_on_paste_is_on() {
    let mut harness = harness("a\nb\n");
    harness.state_mut().config.format_on_paste = true;
    click_end(&mut harness, 1);

    harness.event(Event::Paste("\nc".into()));
    harness.run();

    assert_eq!(content(&harness), "a\n// range\nb\nc\n");
}

/// An editor on `a\nb\nc` whose services give two lenses: places to list above line 0, a
/// command above line 1. Rows: lens 0, line 0, lens 1, line 1, line 2.
fn harness_with_service_lenses() -> Harness<'static, Fixture> {
    let mut harness = harness("a\nb\nc");
    let line_1 = rustext_core::Range::new(Position::new(1, 0), Position::new(1, 1));
    let run = ServiceCommand { name: "test.run".into(), title: "Run".into(), arguments: "[1]".into() };
    harness.state_mut().services.code_lenses = vec![
        CodeLens {
            line: 0,
            label: "1 implementation".into(),
            action: CodeLensAction::ShowLocations(vec![Location::here(line_1)]),
        },
        CodeLens { line: 1, label: "Run".into(), action: CodeLensAction::Run(run) },
    ];
    // Asked at once the first time.
    harness.state_mut().view.service_code_lenses = Default::default();
    harness.run();
    harness
}

#[test]
fn a_service_lens_with_places_lists_them_and_enter_goes_there() {
    let mut harness = harness_with_service_lenses();
    assert_eq!(harness.state().view.service_code_lenses.lenses().len(), 2);

    let lens = harness.state().view.geometry().unwrap().cell_center(0, 1);
    press(&mut harness, lens, Modifiers::NONE);
    assert!(harness.state().view.references.is_open());
    harness.key_press(Key::Enter);
    harness.run();

    assert_eq!(harness.state().state.text().position(harness.state().state.selections().primary().head).line, 1);
}

#[test]
fn a_service_lens_with_a_command_hands_it_to_the_application() {
    let mut harness = harness_with_service_lenses();

    let lens = harness.state().view.geometry().unwrap().cell_center(2, 1);
    press(&mut harness, lens, Modifiers::NONE);

    let names: Vec<&str> = harness.state().service_commands.iter().map(|command| command.name.as_str()).collect();
    assert_eq!(names, vec!["test.run"]);
}

#[test]
fn service_lenses_follow_edits_and_are_asked_again_once_the_text_rests() {
    let mut harness = harness_with_service_lenses();
    let requests_before = harness.state().services.code_lens_requests;
    let lens_lines = |harness: &Harness<'_, Fixture>| -> Vec<usize> {
        harness.state().view.service_code_lenses.lenses().iter().map(|lens| lens.line).collect()
    };
    let line_start = harness.state().view.geometry().unwrap().cell_left(1, 0);
    press(&mut harness, line_start, Modifiers::NONE);

    harness.key_press(Key::Enter);
    harness.step();
    assert_eq!(lens_lines(&harness), vec![1, 2]);
    assert_eq!(harness.state().services.code_lens_requests, requests_before);

    for _ in 0..30 {
        harness.step();
    }
    assert_eq!(lens_lines(&harness), vec![0, 1]);
    assert_eq!(harness.state().services.code_lens_requests, requests_before + 1);
}

#[test]
fn escape_closes_the_references_without_moving() {
    let mut harness = harness("helper();\nhelper();\n");
    let pos = harness.state().view.geometry().unwrap().cell_center(0, 2);
    press(&mut harness, pos, Modifiers::NONE);
    let head_before = harness.state().state.selections().primary().head;

    harness.key_press_modifiers(Modifiers::SHIFT, Key::F12);
    harness.run();
    assert!(harness.state().view.references.is_open());
    harness.key_press(Key::Escape);
    harness.run();
    assert!(!harness.state().view.references.is_open());
    assert_eq!(harness.state().state.selections().primary().head, head_before);
}
