//! A tiny language server for tests: answers `initialize`, completion (two items, one a
//! snippet), hover ("fake hover at line:character on \"its copy of the line\"") and definition (line 0, character 3),
//! with a delay so clients see pending answers; publishes one warning on lines 0:0..0:2 when
//! the document opens or changes.

use std::io::{BufReader, Write};

use rustext_lsp::transport::{read_message, write_message};
use serde_json::{Value, json};

fn main() {
    let mut reader = BufReader::new(std::io::stdin());
    // The document as the server knows it, to check what clients send.
    let mut document: Vec<String> = Vec::new();
    let mut stdout = std::io::stdout();
    while let Some(message) = read_message(&mut reader) {
        let method = message["method"].as_str().unwrap_or_default();
        if method == "textDocument/didOpen" {
            document = message["params"]["textDocument"]["text"]
                .as_str()
                .unwrap_or_default()
                .split('\n')
                .map(str::to_string)
                .collect();
        }
        if method == "textDocument/didChange" {
            for change in message["params"]["contentChanges"].as_array().cloned().unwrap_or_default() {
                apply_change(&mut document, &change);
            }
        }
        if method == "textDocument/didOpen" || method == "textDocument/didChange" {
            let uri = message["params"]["textDocument"]["uri"].clone();
            let diagnostic = json!({ "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 2 } }, "severity": 2, "message": "fake warning" });
            let notification = json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": { "uri": uri, "diagnostics": [diagnostic] } });
            let _ = write_message(&mut stdout, &notification);
            let _ = stdout.flush();
        }
        let id = message.get("id").cloned();
        let position = &message["params"]["position"];
        let result = match method {
            "initialize" => {
                json!({ "capabilities": { "textDocumentSync": 2, "inlayHintProvider": true, "documentHighlightProvider": true, "completionProvider": { "triggerCharacters": ["."] }, "hoverProvider": true, "definitionProvider": true } })
            }
            "fake/document" => json!(document.join("\n")),
            "textDocument/completion" => json!({ "isIncomplete": false, "items": [
                { "label": "fake_completion", "kind": 3, "detail": "fn() -> bool" },
                { "label": "fake_snippet", "kind": 15, "insertText": "fake(${1:arg})", "insertTextFormat": 2 },
            ] }),
            "textDocument/hover" => {
                let line_text =
                    document.get(position["line"].as_u64().unwrap_or(0) as usize).cloned().unwrap_or_default();
                let value = format!("fake hover at {}:{} on {line_text:?}", position["line"], position["character"]);
                json!({ "contents": { "kind": "plaintext", "value": value } })
            }
            "textDocument/definition" => {
                let uri = message["params"]["textDocument"]["uri"].clone();
                json!({ "uri": uri, "range": { "start": { "line": 0, "character": 3 }, "end": { "line": 0, "character": 6 } } })
            }
            "textDocument/rename" => {
                let uri = message["params"]["textDocument"]["uri"].as_str().unwrap_or_default().to_string();
                let new_name = message["params"]["newName"].clone();
                let edit = |line: u64| json!({ "range": { "start": { "line": line, "character": 0 }, "end": { "line": line, "character": 2 } }, "newText": new_name });
                json!({ "changes": { uri: [edit(0), edit(1)] } })
            }
            "textDocument/signatureHelp" => json!({
                "signatures": [{ "label": "fake(first: u8, second: u8)", "parameters": [{ "label": "first: u8" }, { "label": [16, 26] }] }],
                "activeSignature": 0,
                "activeParameter": 1,
            }),
            "textDocument/documentHighlight" => {
                let range = |line: u64| json!({ "start": { "line": line, "character": 0 }, "end": { "line": line, "character": 2 } });
                json!([{ "range": range(0), "kind": 3 }, { "range": range(1), "kind": 2 }])
            }
            "textDocument/inlayHint" => {
                let range = &message["params"]["range"];
                let first = range["start"]["line"].as_u64().unwrap_or(0);
                let end = range["end"]["line"].as_u64().unwrap_or(0);
                let hint = |line: u64| json!({ "position": { "line": line, "character": 2 }, "label": [{ "value": ": " }, { "value": "u8" }], "paddingLeft": true });
                json!((first..end).map(hint).collect::<Vec<_>>())
            }
            "exit" => return,
            _ => Value::Null,
        };
        if let Some(id) = id {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let _ = write_message(&mut stdout, &json!({ "jsonrpc": "2.0", "id": id, "result": result }));
            let _ = stdout.flush();
        }
    }
}

/// Applies an LSP content change (a range and its text, or a whole new text) to the lines.
/// Columns are UTF-16 code units, as LSP says.
fn apply_change(document: &mut Vec<String>, change: &Value) {
    let new_text = change["text"].as_str().unwrap_or_default();
    let Some(range) = change.get("range") else {
        *document = new_text.split('\n').map(str::to_string).collect();
        return;
    };
    let joined = document.join("\n");
    let offset_of = |position: &Value| {
        let line = position["line"].as_u64().unwrap_or(0) as usize;
        let utf16_column = position["character"].as_u64().unwrap_or(0) as usize;
        let mut units = 0;
        let column = document.get(line).map_or(0, |text| {
            text.chars()
                .take_while(|character| {
                    units += character.len_utf16();
                    units <= utf16_column
                })
                .count()
        });
        let line_start: usize = document.iter().take(line).map(|text| text.chars().count() + 1).sum();
        line_start + column
    };
    let (start, end) = (offset_of(&range["start"]), offset_of(&range["end"]));
    let chars: Vec<char> = joined.chars().collect();
    let edited: String = chars[..start].iter().collect::<String>()
        + new_text
        + &chars[end.min(chars.len())..].iter().collect::<String>();
    *document = edited.split('\n').map(str::to_string).collect();
}
