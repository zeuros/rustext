//! Talks to a real language server, rust-analyzer, on a small cargo project: what the fake
//! server can't prove is that real answers parse. Ignored: it needs `rust-analyzer` on the PATH
//! and takes seconds to index.
//! `cargo test -p rustext-lsp --test rust_analyzer -- --ignored`.

use std::time::{Duration, Instant};

use rustext_core::services::{LanguageServices, Reply};
use rustext_core::{Position, Text};
use rustext_lsp::LspServices;

const SOURCE: &str = "struct Point {\n    x: i32,\n}\n\nfn length(point: &Point) -> i32 {\n    point.x\n}\n\nfn main() {\n    let origin = Point { x: 1 };\n    length(&origin);\n}\n";

/// A cargo project holding `SOURCE` as `src/main.rs`; returns the path of that file.
fn project() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("rustext-rust-analyzer-{}", std::process::id()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n")
        .unwrap();
    let main = root.join("src/main.rs");
    std::fs::write(&main, SOURCE).unwrap();
    main
}

/// Asks until the answer is ready and `is_complete` (rust-analyzer answers empty while it
/// indexes), asking again on a new request each time.
fn until_answered<T>(mut ask: impl FnMut(u64) -> Reply<T>, is_complete: impl Fn(&T) -> bool) -> T {
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut attempt = 0;
    loop {
        if let Reply::Ready(answer) = ask(attempt) {
            if is_complete(&answer) {
                return answer;
            }
            attempt += 1;
            std::thread::sleep(Duration::from_millis(200));
        }
        assert!(Instant::now() < deadline, "rust-analyzer gave no complete answer");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[ignore = "needs rust-analyzer; slow"]
#[expect(clippy::cognitive_complexity, clippy::too_many_lines, reason = "predates the complexity lints (D28)")]
fn rust_analyzer_answers_parse() {
    let main = project();
    let mut services =
        LspServices::start("rust-analyzer", &[], &main, "rust", Duration::from_secs(30)).expect("rust-analyzer starts");
    let text = Text::new(SOURCE);
    let call = Position::new(10, 5);

    // Each attempt is a new revision of the same text: a new request, not the cached answer.
    let hover = until_answered(|attempt| services.hover(&text, attempt, call), Option::is_some);
    assert!(hover.unwrap().contains("fn length"));

    let definition = until_answered(|attempt| services.definition(&text, attempt, call), Option::is_some);
    assert_eq!(definition.map(|location| (location.document, location.range.start)), Some((None, Position::new(4, 3))));

    let references = until_answered(|attempt| services.references(&text, attempt, call), |found| found.len() == 2);
    assert_eq!(references.len(), 2);

    let completions =
        until_answered(|attempt| services.completions(&text, attempt, Position::new(5, 10)), |items| !items.is_empty());
    assert!(completions.iter().any(|item| item.label == "x"));

    let hints = until_answered(|attempt| services.inlay_hints(&text, attempt, 0..12), |hints| !hints.is_empty());
    assert!(hints.iter().any(|hint| hint.label.contains("Point")));

    let edits = until_answered(|attempt| services.rename(&text, attempt, call, "size"), |edits| !edits.is_empty());
    assert_eq!(edits.len(), 2);

    let inside_call = Position::new(10, 11);
    let signature = until_answered(|attempt| services.signature_help(&text, attempt, inside_call), Option::is_some);
    let signature = signature.unwrap();
    let (start, end) = signature.active_parameter.unwrap();
    assert_eq!(signature.label.chars().skip(start).take(end - start).collect::<String>(), "point: &Point");

    let highlights = until_answered(
        |attempt| services.document_highlights(&text, attempt, call),
        |ranges| ranges.as_ref().is_some_and(|ranges| ranges.len() == 2),
    );
    assert_eq!(highlights.map(|ranges| ranges.len()), Some(2));

    let symbols = until_answered(|attempt| services.document_symbols(&text, attempt), |symbols| symbols.len() >= 3);
    let names: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
    assert!(names.starts_with(&["Point", "x", "length"]), "{names:?}");

    // rust-analyzer formats with rustfmt.
    let messy =
        Text::new("struct Point {\n    x: i32,\n}\n\nfn  length( point: &Point ) -> i32 { point.x }\n\nfn main() {}\n");
    let indentation = rustext_core::indentation::Indentation { tab_size: 4, insert_spaces: true };
    let edits = until_answered(
        |attempt| services.format_document(&messy, 1_000 + attempt, indentation),
        |edits| !edits.is_empty(),
    );
    let formatted = rustext_core::services::transaction_of_edits(&messy, &edits).unwrap().changes.apply(&messy);
    assert!(formatted.to_string().contains("fn length(point: &Point) -> i32 {\n    point.x\n}"), "{formatted}");

    // rust-analyzer formats on `.` (indents a method call continuing the previous line).
    assert!(services.format_on_type_trigger_chars().contains(&'.'), "{:?}", services.format_on_type_trigger_chars());
    let chain = Text::new("fn main() {\n    let v = Vec::<u8>::new()\n.len();\n}\n");
    let after_dot = Position::new(2, 1);
    let edits = until_answered(
        |attempt| services.format_on_type(&chain, 2_000 + attempt, after_dot, '.', indentation),
        |edits| !edits.is_empty(),
    );
    let formatted = rustext_core::services::transaction_of_edits(&chain, &edits).unwrap().changes.apply(&chain);
    eprintln!("rust-analyzer on type: {:?}", formatted.to_string());
    assert!(formatted.line_content(2).starts_with("        ."), "{formatted}");

    let lenses = until_answered(|attempt| services.code_lenses(&text, 3_000 + attempt), |lenses| !lenses.is_empty());
    let point_lens = lenses.iter().find(|lens| lens.line == 0).expect("a lens above `struct Point`");
    assert_eq!(point_lens.label, "0 implementations");
    assert_eq!(point_lens.action, rustext_core::services::CodeLensAction::ShowLocations(Vec::new()));

    let on_point = rustext_core::Range::new(Position::new(0, 8), Position::new(0, 8));
    let actions =
        until_answered(|attempt| services.code_actions(&text, attempt, on_point), |actions| !actions.is_empty());
    assert!(actions.iter().all(|action| !action.edits.is_empty()));
    eprintln!(
        "rust-analyzer's actions on `Point`: {:?}",
        actions.iter().map(|action| &action.title).collect::<Vec<_>>()
    );

    let _ = std::fs::remove_dir_all(main.parent().unwrap().parent().unwrap());
}
