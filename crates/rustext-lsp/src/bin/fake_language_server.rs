//! A tiny language server for tests: answers `initialize`, completion (two items, one a
//! snippet), hover ("fake hover at line:character") and definition (line 0, character 3),
//! with a delay so clients see pending answers; publishes one warning on lines 0:0..0:2 when
//! the document opens or changes.

use std::io::{BufReader, Write};

use rustext_lsp::transport::{read_message, write_message};
use serde_json::{Value, json};

fn main() {
    let mut reader = BufReader::new(std::io::stdin());
    let mut stdout = std::io::stdout();
    while let Some(message) = read_message(&mut reader) {
        let method = message["method"].as_str().unwrap_or_default();
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
                json!({ "capabilities": { "completionProvider": { "triggerCharacters": ["."] }, "hoverProvider": true, "definitionProvider": true } })
            }
            "textDocument/completion" => json!({ "isIncomplete": false, "items": [
                { "label": "fake_completion", "kind": 3, "detail": "fn() -> bool" },
                { "label": "fake_snippet", "kind": 15, "insertText": "fake(${1:arg})", "insertTextFormat": 2 },
            ] }),
            "textDocument/hover" => {
                json!({ "contents": { "kind": "plaintext", "value": format!("fake hover at {}:{}", position["line"], position["character"]) } })
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
