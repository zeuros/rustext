//! Talks to the fake language server through `LspServices`, as the editor does: asking again
//! while the answer is pending.

use std::time::{Duration, Instant};

use rustext_core::services::{LanguageServices, Reply};
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
    assert_eq!(hover.as_deref(), Some("fake hover at 0:6"));

    let definition = until_ready(|| services.definition(&text, 1, Position::new(1, 0)));
    assert_eq!(definition, Some(Position::new(0, 3)));
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
