//! A language server as `LanguageServices` (DECISIONS D9): requests go out tagged with the text
//! revision and position they ask about; the editor gets `Reply::Pending` until the answer
//! arrives on a later frame. Positions are converted between chars (rustext, D5) and UTF-16
//! (LSP) here only.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use rustext_core::services::{
    CompletionItem, CompletionKind, Diagnostic, LanguageServices, Reply, Severity, SignatureHelp, TextEdit,
};
use rustext_core::{Position, Range, Text};
use serde_json::{Value, json};

use crate::transport::Connection;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum RequestKind {
    Completion,
    Hover,
    Definition,
    Rename,
    SignatureHelp,
}

/// What a request was about: answers for another key are not what the editor asks for now.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RequestKey {
    kind: RequestKind,
    text_revision: u64,
    position: Position,
    /// The new name of a rename.
    argument: String,
}

pub struct LspServices {
    connection: Connection,
    document_uri: String,
    language_id: String,
    synced_revision: Option<u64>,
    document_version: i64,
    trigger_chars: Vec<char>,
    /// The server takes incremental changes (`textDocumentSync.change == 2`).
    syncs_incrementally: bool,
    request_ids: HashMap<RequestKey, u64>,
    results: HashMap<u64, Value>,
    /// The last `publishDiagnostics` for the document, not taken yet.
    published_diagnostics: Option<Vec<Value>>,
}

/// `file:///path` for an absolute path.
pub fn file_uri(path: &std::path::Path) -> String {
    format!("file://{}", path.display())
}

/// Column in UTF-16 code units of char `column` on `line`.
pub fn utf16_column(text: &Text, line: usize, column: usize) -> usize {
    text.line_content(line).chars().take(column).map(char::len_utf16).sum()
}

/// Char column of UTF-16 column `utf16_column` on `line`.
pub fn char_column(text: &Text, line: usize, utf16_column: usize) -> usize {
    let mut units = 0;
    let mut column = 0;
    for character in text.line_content(line).chars() {
        if units >= utf16_column {
            break;
        }
        units += character.len_utf16();
        column += 1;
    }
    column
}

impl LspServices {
    /// Starts `program` as the language server of `file_path` (in language `language_id`),
    /// waiting up to `initialize_timeout` for it to initialize.
    pub fn start(
        program: &str,
        arguments: &[&str],
        file_path: &std::path::Path,
        language_id: &str,
        initialize_timeout: Duration,
    ) -> std::io::Result<Self> {
        let mut connection = Connection::spawn(program, arguments)?;
        let root_uri = file_path.parent().map(file_uri);
        let capabilities = json!({
            "textDocument": {
                "completion": { "completionItem": { "snippetSupport": true } },
                "hover": { "contentFormat": ["plaintext", "markdown"] },
                "synchronization": { "didSave": false },
            }
        });
        let initialize_id = connection.request(
            "initialize",
            json!({ "processId": std::process::id(), "rootUri": root_uri, "capabilities": capabilities }),
        )?;
        let deadline = Instant::now() + initialize_timeout;
        let initialize_result = loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let message = connection.incoming.recv_timeout(remaining).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "the language server did not initialize")
            })?;
            if message["id"] == json!(initialize_id) {
                break message["result"].clone();
            }
        };
        let trigger_chars = initialize_result["capabilities"]["completionProvider"]["triggerCharacters"]
            .as_array()
            .map(|chars| chars.iter().filter_map(|value| value.as_str()?.chars().next()).collect())
            .unwrap_or_default();
        let sync = &initialize_result["capabilities"]["textDocumentSync"];
        let syncs_incrementally = sync.as_u64().or(sync["change"].as_u64()) == Some(2);
        connection.notify("initialized", json!({}))?;
        Ok(Self {
            connection,
            document_uri: file_uri(file_path),
            language_id: language_id.to_string(),
            synced_revision: None,
            document_version: 0,
            trigger_chars,
            syncs_incrementally,
            request_ids: HashMap::new(),
            results: HashMap::new(),
            published_diagnostics: None,
        })
    }

    /// Sends the text when it changed since the last sync (full sync).
    fn sync(&mut self, text: &Text, text_revision: u64) {
        if self.synced_revision == Some(text_revision) {
            return;
        }
        self.document_version += 1;
        let (uri, version) = (self.document_uri.clone(), self.document_version);
        let _ = if self.synced_revision.is_none() {
            let item =
                json!({ "uri": uri, "languageId": self.language_id, "version": version, "text": text.to_string() });
            self.connection.notify("textDocument/didOpen", json!({ "textDocument": item }))
        } else {
            let params = json!({ "textDocument": { "uri": uri, "version": version }, "contentChanges": [{ "text": text.to_string() }] });
            self.connection.notify("textDocument/didChange", params)
        };
        self.synced_revision = Some(text_revision);
        self.request_ids.retain(|key, _| key.text_revision == text_revision);
    }

    /// Takes what the server sent: results by request id; its own requests get a null result.
    fn receive(&mut self) {
        while let Ok(message) = self.connection.incoming.try_recv() {
            let id = message["id"].as_u64();
            match (id, message.get("method")) {
                (Some(id), None) => {
                    self.results.insert(id, message.get("result").cloned().unwrap_or(Value::Null));
                }
                (Some(_), Some(_)) => {
                    let _ = self.connection.respond(message["id"].clone(), Value::Null);
                }
                (None, Some(method))
                    if method == "textDocument/publishDiagnostics"
                        && message["params"]["uri"].as_str() == Some(self.document_uri.as_str()) =>
                {
                    self.published_diagnostics = message["params"]["diagnostics"].as_array().cloned();
                }
                _ => {}
            }
        }
    }

    /// The result of the request for `key`, sending it the first time.
    fn ask(&mut self, text: &Text, key: RequestKey, method: &str) -> Reply<Value> {
        self.ask_with(text, key, method, json!({}))
    }

    /// `ask` with more request parameters (merged into the position parameters).
    fn ask_with(&mut self, text: &Text, key: RequestKey, method: &str, extra_params: Value) -> Reply<Value> {
        self.sync(text, key.text_revision);
        self.receive();
        if let Some(&id) = self.request_ids.get(&key) {
            return match self.results.remove(&id) {
                Some(result) => {
                    self.request_ids.remove(&key);
                    Reply::Ready(result)
                }
                None => Reply::Pending,
            };
        }
        let line = key.position.line;
        let mut params = json!({
            "textDocument": { "uri": self.document_uri },
            "position": { "line": line, "character": utf16_column(text, line, key.position.column) },
        });
        if let (Some(params), Some(extra)) = (params.as_object_mut(), extra_params.as_object()) {
            params.extend(extra.clone());
        }
        if let Ok(id) = self.connection.request(method, params) {
            self.request_ids.insert(key, id);
        }
        Reply::Pending
    }
}

fn completion_kind(lsp_kind: u64) -> CompletionKind {
    match lsp_kind {
        2 => CompletionKind::Method,
        3 | 4 => CompletionKind::Function,
        5 | 10 => CompletionKind::Field,
        6 => CompletionKind::Variable,
        7 | 8 | 13 | 22 | 25 => CompletionKind::Type,
        9 => CompletionKind::Module,
        14 => CompletionKind::Keyword,
        15 => CompletionKind::Snippet,
        12 | 20 | 21 => CompletionKind::Constant,
        _ => CompletionKind::Text,
    }
}

fn completion_items(result: &Value) -> Vec<CompletionItem> {
    let items = result.get("items").unwrap_or(result).as_array().cloned().unwrap_or_default();
    items
        .iter()
        .filter_map(|item| {
            let label = item["label"].as_str()?.to_string();
            let insert_text = item["textEdit"]["newText"].as_str().or(item["insertText"].as_str()).map(str::to_string);
            Some(CompletionItem {
                kind: completion_kind(item["kind"].as_u64().unwrap_or(1)),
                detail: item["detail"].as_str().map(str::to_string),
                is_snippet: item["insertTextFormat"].as_u64() == Some(2),
                insert_text,
                label,
            })
        })
        .collect()
}

/// Hover contents as text: a string, `{kind, value}`, a `MarkedString`, or a list of them.
fn hover_text(contents: &Value) -> Option<String> {
    match contents {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let texts: Vec<String> = parts.iter().filter_map(hover_text).collect();
            (!texts.is_empty()).then(|| texts.join("\n\n"))
        }
        Value::Object(object) => object.get("value").and_then(Value::as_str).map(str::to_string),
        _ => None,
    }
}

impl LanguageServices for LspServices {
    fn completions(&mut self, text: &Text, text_revision: u64, position: Position) -> Reply<Vec<CompletionItem>> {
        let key = RequestKey { kind: RequestKind::Completion, text_revision, position, argument: String::new() };
        match self.ask(text, key, "textDocument/completion") {
            Reply::Ready(result) => Reply::Ready(completion_items(&result)),
            Reply::Pending => Reply::Pending,
        }
    }

    fn take_diagnostics(&mut self, text: &Text) -> Option<Vec<Diagnostic>> {
        self.receive();
        let published = self.published_diagnostics.take()?;
        let position = |value: &Value| -> Option<Position> {
            let line = (value["line"].as_u64()? as usize).min(text.last_line());
            Some(Position::new(line, char_column(text, line, value["character"].as_u64()? as usize)))
        };
        let diagnostics = published
            .iter()
            .filter_map(|item| {
                let range = Range::new(position(&item["range"]["start"])?, position(&item["range"]["end"])?);
                let severity = match item["severity"].as_u64() {
                    Some(2) => Severity::Warning,
                    Some(3) => Severity::Info,
                    Some(4) => Severity::Hint,
                    _ => Severity::Error,
                };
                let message = item["message"].as_str().unwrap_or_default().to_string();
                Some(Diagnostic { range, severity, message })
            })
            .collect();
        Some(diagnostics)
    }

    fn rename(&mut self, text: &Text, text_revision: u64, position: Position, new_name: &str) -> Reply<Vec<TextEdit>> {
        let key = RequestKey { kind: RequestKind::Rename, text_revision, position, argument: new_name.to_string() };
        let result = match self.ask_with(text, key, "textDocument/rename", json!({ "newName": new_name })) {
            Reply::Ready(result) => result,
            Reply::Pending => return Reply::Pending,
        };
        let uri = self.document_uri.clone();
        let edits_of_document = result["changes"][uri.as_str()].as_array().cloned().or_else(|| {
            let changes = result["documentChanges"].as_array()?;
            let change = changes.iter().find(|change| change["textDocument"]["uri"].as_str() == Some(uri.as_str()))?;
            change["edits"].as_array().cloned()
        });
        let position_of = |value: &Value| -> Option<Position> {
            let line = value["line"].as_u64()? as usize;
            Some(Position::new(line, char_column(text, line, value["character"].as_u64()? as usize)))
        };
        let edits = edits_of_document
            .unwrap_or_default()
            .iter()
            .filter_map(|edit| {
                let range = Range::new(position_of(&edit["range"]["start"])?, position_of(&edit["range"]["end"])?);
                Some(TextEdit { range, new_text: edit["newText"].as_str()?.to_string() })
            })
            .collect();
        Reply::Ready(edits)
    }

    fn signature_help(&mut self, text: &Text, text_revision: u64, position: Position) -> Reply<Option<SignatureHelp>> {
        let key = RequestKey { kind: RequestKind::SignatureHelp, text_revision, position, argument: String::new() };
        let result = match self.ask(text, key, "textDocument/signatureHelp") {
            Reply::Ready(result) => result,
            Reply::Pending => return Reply::Pending,
        };
        let signature_index = result["activeSignature"].as_u64().unwrap_or(0) as usize;
        let Some(signature) = result["signatures"].get(signature_index) else { return Reply::Ready(None) };
        let label = signature["label"].as_str().unwrap_or_default().to_string();
        let parameter_index =
            signature["activeParameter"].as_u64().or(result["activeParameter"].as_u64()).unwrap_or(0) as usize;
        let active_parameter =
            signature["parameters"].get(parameter_index).and_then(|parameter| match &parameter["label"] {
                Value::String(parameter_label) => {
                    let byte = label.find(parameter_label.as_str())?;
                    let start = label[..byte].chars().count();
                    Some((start, start + parameter_label.chars().count()))
                }
                Value::Array(offsets) => {
                    let utf16_to_chars = |units: u64| {
                        let mut seen = 0;
                        label
                            .chars()
                            .take_while(|character| {
                                seen += character.len_utf16() as u64;
                                seen <= units
                            })
                            .count()
                    };
                    Some((utf16_to_chars(offsets.first()?.as_u64()?), utf16_to_chars(offsets.get(1)?.as_u64()?)))
                }
                _ => None,
            });
        let documentation = hover_text(&signature["documentation"]);
        Reply::Ready(Some(SignatureHelp { label, active_parameter, documentation }))
    }

    /// Sends the edits as ranges (in reverse order, so each is relative to the text after the
    /// previous one) when the server takes them; otherwise the next request syncs in full.
    fn text_changed(&mut self, texts_before: &[Text], changes: &[rustext_core::ChangeSet], text_revision_after: u64) {
        if !self.syncs_incrementally || self.synced_revision.is_none() || texts_before.len() != changes.len() {
            return;
        }
        let position = |text: &Text, offset: usize| {
            let position = text.position(offset);
            json!({ "line": position.line, "character": utf16_column(text, position.line, position.column) })
        };
        let content_changes: Vec<Value> = texts_before
            .iter()
            .zip(changes)
            .flat_map(|(before, change_set)| {
                change_set
                    .changes()
                    .iter()
                    .rev()
                    .map(|change| json!({ "range": { "start": position(before, change.start), "end": position(before, change.end) }, "text": change.insert }))
                    .collect::<Vec<_>>()
            })
            .collect();
        self.document_version += 1;
        let params = json!({ "textDocument": { "uri": self.document_uri, "version": self.document_version }, "contentChanges": content_changes });
        if self.connection.notify("textDocument/didChange", params).is_ok() {
            self.synced_revision = Some(text_revision_after);
        }
    }

    fn completion_trigger_chars(&self) -> &[char] {
        &self.trigger_chars
    }

    fn hover(&mut self, text: &Text, text_revision: u64, position: Position) -> Reply<Option<String>> {
        let key = RequestKey { kind: RequestKind::Hover, text_revision, position, argument: String::new() };
        match self.ask(text, key, "textDocument/hover") {
            Reply::Ready(result) => {
                Reply::Ready(hover_text(&result["contents"]).filter(|text| !text.trim().is_empty()))
            }
            Reply::Pending => Reply::Pending,
        }
    }

    fn definition(&mut self, text: &Text, text_revision: u64, position: Position) -> Reply<Option<Position>> {
        let key = RequestKey { kind: RequestKind::Definition, text_revision, position, argument: String::new() };
        let result = match self.ask(text, key, "textDocument/definition") {
            Reply::Ready(result) => result,
            Reply::Pending => return Reply::Pending,
        };
        let locations = match &result {
            Value::Array(locations) => locations.clone(),
            Value::Null => Vec::new(),
            location => vec![location.clone()],
        };
        let in_this_document = locations.iter().find(|location| {
            let uri = location.get("uri").or(location.get("targetUri"));
            uri.and_then(Value::as_str) == Some(self.document_uri.as_str())
        });
        let start = in_this_document.map(|location| {
            location.get("range").or(location.get("targetSelectionRange")).map_or(&Value::Null, |range| &range["start"])
        });
        Reply::Ready(start.and_then(|start| {
            let line = start["line"].as_u64()? as usize;
            Some(Position::new(line, char_column(text, line, start["character"].as_u64()? as usize)))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_convert_between_chars_and_utf16() {
        let text = Text::new("a😀b");

        assert_eq!(utf16_column(&text, 0, 2), 3);
        assert_eq!(char_column(&text, 0, 3), 2);
    }

    #[test]
    fn reads_completion_lists_and_arrays() {
        let list =
            json!({ "isIncomplete": false, "items": [{ "label": "push", "kind": 2, "detail": "fn(&mut self, T)" }] });
        let array = json!([{ "label": "fn", "kind": 15, "insertText": "fn ${1:name}() {}", "insertTextFormat": 2 }]);

        assert_eq!(completion_items(&list)[0].kind, CompletionKind::Method);
        let snippet = &completion_items(&array)[0];
        assert!(snippet.is_snippet);
        assert_eq!(snippet.text_to_insert(), "fn ${1:name}() {}");
    }

    #[test]
    fn reads_hover_contents_in_every_shape() {
        assert_eq!(hover_text(&json!("plain")), Some("plain".into()));
        assert_eq!(hover_text(&json!({ "kind": "markdown", "value": "**bold**" })), Some("**bold**".into()));
        assert_eq!(
            hover_text(&json!([{ "language": "rust", "value": "fn a()" }, "doc"])),
            Some("fn a()\n\ndoc".into())
        );
    }
}
