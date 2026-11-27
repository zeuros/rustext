//! Talks to the fake language server through `LspServices`, as the editor does: asking again
//! while the answer is pending.

use std::time::{Duration, Instant};

use rustext_core::services::{CodeLensAction, LanguageServices, Reply};
use rustext_core::{Position, Text};
use rustext_lsp::LspServices;

fn start() -> LspServices {
    let program = env!("CARGO_BIN_EXE_rustext-fake-language-server");
    LspServices::start(program, &[], std::path::Path::new("/tmp/project/main.rs"), "rust", Duration::from_secs(5))
        .expect("server starts")
}

/// Asks until the answer is ready, like the editor does frame after frame.
fn until_ready<T>(mut ask: impl FnMut() -> Reply<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Reply::Ready(answer) = ask() {
            return answer;
        }
        assert!(Instant::now() < deadline, "no answer");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn completions_hover_and_definition_from_a_language_server() {
    let mut services = start();
    let text = Text::new("fn 😀ab() {}\nab");
    assert_eq!(services.completion_trigger_chars(), &['.']);

    let items = until_ready(|| services.completions(&text, 1, Position::new(1, 2)));
    assert_eq!(items.len(), 2);
    assert!(items[1].is_snippet);

    // Char column 5 is after the emoji: UTF-16 column 6 for the server.
    let hover = until_ready(|| services.hover(&text, 1, Position::new(0, 5)));
    assert_eq!(hover.as_deref(), Some("fake hover at 0:6 on \"fn 😀ab() {}\""));

    let definition = until_ready(|| services.definition(&text, 1, Position::new(1, 0)));
    assert_eq!(definition.map(|location| (location.document, location.range.start)), Some((None, Position::new(0, 3))));
}

#[test]
fn the_first_ask_is_pending() {
    let mut services = start();

    assert_eq!(services.completions(&Text::new("x"), 1, Position::new(0, 1)), Reply::Pending);
}

#[test]
fn published_diagnostics_are_taken_once() {
    let mut services = start();
    let text = Text::new("fn main() {}");
    let _ = services.completions(&text, 1, Position::new(0, 0));

    let diagnostics = until_ready(|| match services.take_diagnostics(&text) {
        Some(diagnostics) => Reply::Ready(diagnostics),
        None => Reply::Pending,
    });

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].message, "fake warning");
    assert_eq!(diagnostics[0].severity, rustext_core::services::Severity::Warning);
    assert!(services.take_diagnostics(&text).is_none());
}

#[test]
fn rename_and_signature_help() {
    let mut services = start();
    let text = Text::new("ab\nab c\n");

    let edits = until_ready(|| services.rename(&text, 1, Position::new(0, 0), "xyz"));
    assert_eq!(edits.len(), 2);
    assert_eq!(edits[1].new_text, "xyz");
    assert_eq!(edits[1].range.start, Position::new(1, 0));

    let help = until_ready(|| services.signature_help(&text, 1, Position::new(1, 3))).expect("a signature");
    assert_eq!(help.label, "fake(first: u8, second: u8)");
    assert_eq!(help.active_parameter, Some((16, 26)));
}

#[test]
fn edits_reach_the_server_as_ranges() {
    use rustext_core::{Change, ChangeSet};
    let mut services = start();
    let before = Text::new("one\ntwo 😀 three\n");
    let _ = until_ready(|| services.hover(&before, 1, Position::new(0, 0)));

    // Two edits in one change set, after an emoji (UTF-16 columns differ from char columns).
    let changes = ChangeSet::new([Change::insert(before.offset(Position::new(1, 6)), "big "), Change::new(0, 3, "1")]);
    let after = changes.apply(&before);
    services.text_changed(std::slice::from_ref(&before), std::slice::from_ref(&changes), 2);

    let hover = until_ready(|| services.hover(&after, 2, Position::new(1, 0)));
    assert_eq!(hover.as_deref(), Some("fake hover at 1:0 on \"two 😀 big three\""));
    let hover = until_ready(|| services.hover(&after, 2, Position::new(0, 0)));
    assert_eq!(hover.as_deref(), Some("fake hover at 0:0 on \"1\""));
}

#[test]
fn inlay_hints_of_a_line_range() {
    let mut services = start();
    let text = Text::new("😀b\nab\nab\n");

    let hints = until_ready(|| services.inlay_hints(&text, 1, 0..2));

    assert_eq!(hints.len(), 2);
    assert_eq!(hints[0].label, " : u8");
    assert_eq!(hints[0].position, Position::new(0, 1));
    assert_eq!(hints[1].position, Position::new(1, 2));
}

#[test]
fn document_highlights() {
    let mut services = start();
    let text = Text::new("ab\nab c\n");

    let ranges = until_ready(|| services.document_highlights(&text, 1, Position::new(0, 1))).expect("highlights");

    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[1], rustext_core::Range::new(Position::new(1, 0), Position::new(1, 2)));
}

#[test]
fn references_in_this_document_and_others() {
    let mut services = start();
    let text = Text::new("ab\nab c\n");

    let locations = until_ready(|| services.references(&text, 1, Position::new(0, 1)));

    let found: Vec<(Option<&str>, usize)> =
        locations.iter().map(|location| (location.document.as_deref(), location.range.start.line)).collect();
    assert_eq!(found, vec![(None, 0), (Some("file:///elsewhere.rs"), 5), (None, 1)]);
}

#[test]
fn document_symbols_are_flattened_with_their_containers() {
    let mut services = start();
    let text = Text::new("ab\nab c\n");

    let symbols = until_ready(|| services.document_symbols(&text, 1));

    let found: Vec<(&str, Option<&str>, usize)> = symbols
        .iter()
        .map(|symbol| (symbol.name.as_str(), symbol.container.as_deref(), symbol.range.start.line))
        .collect();
    assert_eq!(found, vec![("Shape", None, 0), ("area", Some("Shape"), 1)]);
}

#[test]
fn formatting_sends_the_indentation_and_reads_the_edits() {
    let mut services = start();
    let text = Text::new("ab\n");
    let indentation = rustext_core::indentation::Indentation { tab_size: 2, insert_spaces: true };

    let edits = until_ready(|| services.format_document(&text, 1, indentation));

    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "// tab size 2\n");
}

#[test]
fn formatting_on_type_sends_the_typed_char_and_its_position() {
    let mut services = start();
    let text = Text::new("fn f() {\n    a;\n}");
    let indentation = rustext_core::indentation::Indentation::default();

    let edits = until_ready(|| services.format_on_type(&text, 1, Position::new(1, 6), ';', indentation));

    assert_eq!(services.format_on_type_trigger_chars(), &['}', ';', '\n']);
    assert_eq!(edits[0].new_text, "// typed ; at 1:6\n");
}

#[test]
fn code_lenses_are_resolved_and_locations_are_listed() {
    let mut services = start();
    let text = Text::new("ab\ncd\n");

    let lenses = until_ready(|| services.code_lenses(&text, 1));

    assert_eq!(lenses.len(), 2);
    assert_eq!((lenses[0].line, lenses[0].label.as_str()), (0, "Run"));
    let CodeLensAction::Run(command) = &lenses[0].action else { panic!("{:?}", lenses[0].action) };
    assert_eq!((command.name.as_str(), command.arguments.as_str()), ("fake.run", r#"[{"test":"first"}]"#));
    assert_eq!((lenses[1].line, lenses[1].label.as_str()), (1, "7 implementations"));
    let CodeLensAction::ShowLocations(locations) = &lenses[1].action else { panic!("{:?}", lenses[1].action) };
    assert_eq!(locations[0].document.as_deref(), Some("file:///elsewhere.rs"));
}

#[test]
fn code_actions_with_edits_come_preferred_first() {
    let mut services = start();
    let text = Text::new("ab\n");
    let cursor = rustext_core::Range::new(Position::new(0, 1), Position::new(0, 1));

    let actions = until_ready(|| services.code_actions(&text, 1, cursor));

    let titles: Vec<&str> = actions.iter().map(|action| action.title.as_str()).collect();
    assert_eq!(titles, vec!["Fix it", "Extract"]);
    assert!(actions[0].is_quick_fix && actions[0].is_preferred);
    assert_eq!(actions[0].edits[0].new_text, "// fixed\n");
}

#[test]
fn range_formatting_sends_the_range() {
    let mut services = start();
    let text = Text::new("a\nb\n");
    let indentation = rustext_core::indentation::Indentation { tab_size: 4, insert_spaces: true };
    let line_one = rustext_core::Range::new(Position::new(1, 0), Position::new(1, 1));

    let edits = until_ready(|| services.format_range(&text, 1, line_one, indentation));

    assert_eq!((edits[0].range.start.line, edits[0].new_text.as_str()), (1, "// range\n"));
}
