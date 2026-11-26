//! A language server as `LanguageServices` (DECISIONS D9): requests go out tagged with the text
//! revision and position they ask about; the editor gets `Reply::Pending` until the answer
//! arrives on a later frame. Messages are `lsp-types` values (D23); positions are converted
//! between chars (rustext, D5) and UTF-16 (LSP) here only.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use lsp_types::notification::{self, Notification};
use lsp_types::request::{self, Request};
use lsp_types::{self as lsp, OneOf, Uri};
use rustext_core::services::{
    CodeAction, CompletionItem, CompletionKind, Diagnostic, DocumentSymbol, InlayHint, LanguageServices, Location,
    Reply, Severity, SignatureHelp, SymbolKind, TextEdit,
};
use rustext_core::{ChangeSet, Position, Range, Text};
use serde_json::Value;

use crate::transport::Connection;

/// What a request was about: answers for another key are not what the editor asks for now.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RequestKey {
    method: &'static str,
    text_revision: u64,
    position: Position,
    /// What else tells two requests apart: the new name of a rename, the end of a line range.
    argument: String,
}

impl RequestKey {
    fn new<R: Request>(text_revision: u64, position: Position) -> Self {
        Self { method: R::METHOD, text_revision, position, argument: String::new() }
    }

    fn with_argument(self, argument: impl Into<String>) -> Self {
        Self { argument: argument.into(), ..self }
    }
}

pub struct LspServices {
    connection: Connection,
    document_uri: Uri,
    language_id: String,
    synced_revision: Option<u64>,
    document_version: i32,
    trigger_chars: Vec<char>,
    /// The server takes incremental changes.
    syncs_incrementally: bool,
    gives_inlay_hints: bool,
    gives_document_highlights: bool,
    request_ids: HashMap<RequestKey, u64>,
    results: HashMap<u64, Value>,
    /// The last `publishDiagnostics` for the document, not taken yet.
    published_diagnostics: Option<Vec<lsp::Diagnostic>>,
}

/// `file:///path` for an absolute path, percent-encoding what a URI path can't hold (spaces).
pub fn file_uri(path: &std::path::Path) -> String {
    let encoded: String = path
        .to_string_lossy()
        .bytes()
        .map(|byte| match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'/' | b'-' | b'.' | b'_' | b'~' => (byte as char).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect();
    format!("file://{encoded}")
}

/// The path of a `file://` URI, percent-decoded; `None` for another scheme.
pub fn file_path_of(uri: &str) -> Option<std::path::PathBuf> {
    let encoded = uri.strip_prefix("file://")?.as_bytes();
    let mut decoded = Vec::with_capacity(encoded.len());
    let mut index = 0;
    while index < encoded.len() {
        let escaped = (encoded[index] == b'%')
            .then(|| std::str::from_utf8(encoded.get(index + 1..index + 3)?).ok())
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                decoded.push(byte);
                index += 3;
            }
            None => {
                decoded.push(encoded[index]);
                index += 1;
            }
        }
    }
    Some(std::path::PathBuf::from(String::from_utf8(decoded).ok()?))
}

/// Turns LSP locations into rustext's: this document's columns from its text, another
/// document's from its file, read once (`None` when it can't be: its columns stay as sent).
struct LocationConverter<'text> {
    text: &'text Text,
    document_uri: &'text Uri,
    other_texts: HashMap<Uri, Option<Text>>,
}

impl<'text> LocationConverter<'text> {
    fn new(text: &'text Text, document_uri: &'text Uri) -> Self {
        Self { text, document_uri, other_texts: HashMap::new() }
    }

    fn location(&mut self, uri: Uri, range: lsp::Range) -> Location {
        if uri == *self.document_uri {
            return Location::here(range_of(self.text, range));
        }
        let other_text = self.other_texts.entry(uri.clone()).or_insert_with(|| {
            let content = std::fs::read_to_string(file_path_of(uri.as_str())?).ok()?;
            Some(Text::new(&content))
        });
        let range = match other_text {
            Some(other_text) => range_of(other_text, range),
            None => Range::new(
                Position::new(range.start.line as usize, range.start.character as usize),
                Position::new(range.end.line as usize, range.end.character as usize),
            ),
        };
        Location::in_document(uri.as_str(), range)
    }
}

fn document_uri(path: &std::path::Path) -> std::io::Result<Uri> {
    Uri::from_str(&file_uri(path)).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))
}

/// Chars in the first `utf16_units` UTF-16 code units of `content`.
fn chars_in_utf16_prefix(content: &str, utf16_units: usize) -> usize {
    let mut units = 0;
    content
        .chars()
        .take_while(|character| {
            let starts_inside = units < utf16_units;
            units += character.len_utf16();
            starts_inside
        })
        .count()
}

/// Column in UTF-16 code units of char `column` on `line`.
pub fn utf16_column(text: &Text, line: usize, column: usize) -> usize {
    text.line_content(line).chars().take(column).map(char::len_utf16).sum()
}

/// Char column of UTF-16 column `utf16_column` on `line`.
pub fn char_column(text: &Text, line: usize, utf16_column: usize) -> usize {
    chars_in_utf16_prefix(&text.line_content(line), utf16_column)
}

fn lsp_position(text: &Text, position: Position) -> lsp::Position {
    lsp::Position::new(position.line as u32, utf16_column(text, position.line, position.column) as u32)
}

fn lsp_position_of_offset(text: &Text, offset: usize) -> lsp::Position {
    lsp_position(text, text.position(offset))
}

/// The position an LSP position names, its line clamped to the text (a diagnostic at the end
/// of the file may name the line after the last).
fn position_of(text: &Text, position: lsp::Position) -> Position {
    let line = (position.line as usize).min(text.last_line());
    Position::new(line, char_column(text, line, position.character as usize))
}

fn range_of(text: &Text, range: lsp::Range) -> Range {
    Range::new(position_of(text, range.start), position_of(text, range.end))
}

/// Whether a capability announced as `true` or as options is there.
fn is_announced<Options>(provider: &Option<OneOf<bool, Options>>) -> bool {
    !matches!(provider, None | Some(OneOf::Left(false)))
}

fn client_capabilities() -> lsp::ClientCapabilities {
    let completion_item = lsp::CompletionItemCapability { snippet_support: Some(true), ..Default::default() };
    let signature_information = lsp::SignatureInformationSettings {
        parameter_information: Some(lsp::ParameterInformationSettings { label_offset_support: Some(true) }),
        active_parameter_support: Some(true),
        ..Default::default()
    };
    lsp::ClientCapabilities {
        text_document: Some(lsp::TextDocumentClientCapabilities {
            synchronization: Some(lsp::TextDocumentSyncClientCapabilities {
                did_save: Some(false),
                ..Default::default()
            }),
            completion: Some(lsp::CompletionClientCapabilities {
                completion_item: Some(completion_item),
                ..Default::default()
            }),
            hover: Some(lsp::HoverClientCapabilities {
                content_format: Some(vec![lsp::MarkupKind::PlainText, lsp::MarkupKind::Markdown]),
                ..Default::default()
            }),
            signature_help: Some(lsp::SignatureHelpClientCapabilities {
                signature_information: Some(signature_information),
                ..Default::default()
            }),
            rename: Some(Default::default()),
            inlay_hint: Some(Default::default()),
            document_highlight: Some(Default::default()),
            references: Some(Default::default()),
            code_action: Some(lsp::CodeActionClientCapabilities {
                code_action_literal_support: Some(lsp::CodeActionLiteralSupport {
                    code_action_kind: lsp::CodeActionKindLiteralSupport {
                        value_set: [
                            "quickfix",
                            "refactor",
                            "refactor.extract",
                            "refactor.inline",
                            "refactor.rewrite",
                            "source",
                        ]
                        .map(String::from)
                        .to_vec(),
                    },
                }),
                is_preferred_support: Some(true),
                ..Default::default()
            }),
            document_symbol: Some(lsp::DocumentSymbolClientCapabilities {
                hierarchical_document_symbol_support: Some(true),
                ..Default::default()
            }),
            definition: Some(Default::default()),
            publish_diagnostics: Some(Default::default()),
            ..Default::default()
        }),
        ..Default::default()
    }
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
        let root = file_path.parent().map(document_uri).transpose()?;
        let workspace_folders = root.clone().map(|uri| {
            let name = file_path.parent().and_then(|parent| parent.file_name()).unwrap_or_default();
            vec![lsp::WorkspaceFolder { uri, name: name.to_string_lossy().into_owned() }]
        });
        #[allow(deprecated)] // `rootUri`, for servers older than workspace folders.
        let params = lsp::InitializeParams {
            process_id: Some(std::process::id()),
            root_uri: root,
            workspace_folders,
            capabilities: client_capabilities(),
            client_info: Some(lsp::ClientInfo { name: "rustext".into(), version: None }),
            ..Default::default()
        };
        let initialize_id = connection.request(request::Initialize::METHOD, serde_json::to_value(params)?)?;
        let deadline = Instant::now() + initialize_timeout;
        let initialize_result = loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let message = connection.incoming.recv_timeout(remaining).map_err(|error| match error {
                RecvTimeoutError::Timeout => {
                    std::io::Error::new(std::io::ErrorKind::TimedOut, "the language server did not initialize")
                }
                RecvTimeoutError::Disconnected => std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "the language server exited before initializing",
                ),
            })?;
            if message["id"] == initialize_id {
                break serde_json::from_value::<lsp::InitializeResult>(message["result"].clone())?;
            }
        };
        let capabilities = initialize_result.capabilities;
        let trigger_chars = capabilities
            .completion_provider
            .and_then(|provider| provider.trigger_characters)
            .map(|triggers| triggers.iter().filter_map(|trigger| trigger.chars().next()).collect())
            .unwrap_or_default();
        let syncs_incrementally = match capabilities.text_document_sync {
            Some(lsp::TextDocumentSyncCapability::Kind(kind)) => kind == lsp::TextDocumentSyncKind::INCREMENTAL,
            Some(lsp::TextDocumentSyncCapability::Options(options)) => {
                options.change == Some(lsp::TextDocumentSyncKind::INCREMENTAL)
            }
            None => false,
        };
        connection.notify(notification::Initialized::METHOD, serde_json::to_value(lsp::InitializedParams {})?)?;
        Ok(Self {
            connection,
            document_uri: document_uri(file_path)?,
            language_id: language_id.to_string(),
            synced_revision: None,
            document_version: 0,
            trigger_chars,
            syncs_incrementally,
            gives_inlay_hints: is_announced(&capabilities.inlay_hint_provider),
            gives_document_highlights: is_announced(&capabilities.document_highlight_provider),
            request_ids: HashMap::new(),
            results: HashMap::new(),
            published_diagnostics: None,
        })
    }

    fn notify<N: Notification>(&mut self, params: N::Params) -> std::io::Result<()> {
        self.connection.notify(N::METHOD, serde_json::to_value(params)?)
    }

    fn text_document(&self) -> lsp::TextDocumentIdentifier {
        lsp::TextDocumentIdentifier::new(self.document_uri.clone())
    }

    fn text_document_position(&self, text: &Text, position: Position) -> lsp::TextDocumentPositionParams {
        lsp::TextDocumentPositionParams::new(self.text_document(), lsp_position(text, position))
    }

    /// Sends the text when it changed since the last sync (full sync).
    fn sync(&mut self, text: &Text, text_revision: u64) {
        if self.synced_revision == Some(text_revision) {
            return;
        }
        self.document_version += 1;
        let (uri, version) = (self.document_uri.clone(), self.document_version);
        let _ = if self.synced_revision.is_none() {
            let item = lsp::TextDocumentItem::new(uri, self.language_id.clone(), version, text.to_string());
            self.notify::<notification::DidOpenTextDocument>(lsp::DidOpenTextDocumentParams { text_document: item })
        } else {
            let whole_text =
                lsp::TextDocumentContentChangeEvent { range: None, range_length: None, text: text.to_string() };
            self.notify::<notification::DidChangeTextDocument>(lsp::DidChangeTextDocumentParams {
                text_document: lsp::VersionedTextDocumentIdentifier::new(uri, version),
                content_changes: vec![whole_text],
            })
        };
        self.synced_revision = Some(text_revision);
        self.request_ids.retain(|key, _| key.text_revision == text_revision);
    }

    /// Takes what the server sent: results by request id, the document's diagnostics. The
    /// server's own requests get empty answers.
    fn receive(&mut self) {
        while let Ok(message) = self.connection.incoming.try_recv() {
            match (message["id"].as_u64(), message["method"].as_str()) {
                (Some(id), None) => {
                    self.results.insert(id, message.get("result").cloned().unwrap_or(Value::Null));
                }
                (_, Some(request::WorkspaceConfiguration::METHOD)) => {
                    let item_count = message["params"]["items"].as_array().map_or(0, Vec::len);
                    let _ = self.connection.respond(message["id"].clone(), Value::Array(vec![Value::Null; item_count]));
                }
                (None, Some(notification::PublishDiagnostics::METHOD)) => {
                    let Ok(published) =
                        serde_json::from_value::<lsp::PublishDiagnosticsParams>(message["params"].clone())
                    else {
                        continue;
                    };
                    if published.uri == self.document_uri {
                        self.published_diagnostics = Some(published.diagnostics);
                    }
                }
                (None, Some(_)) => {}
                (Some(_), Some(_)) => {
                    let _ = self.connection.respond(message["id"].clone(), Value::Null);
                }
                (None, None) => {}
            }
        }
    }

    /// The result of request `R` for `key`, sending it with `params` the first time. A result
    /// that doesn't parse counts as empty.
    fn ask<R: Request>(&mut self, text: &Text, key: RequestKey, params: R::Params) -> Reply<R::Result>
    where
        R::Result: Default,
    {
        self.sync(text, key.text_revision);
        self.receive();
        if let Some(&id) = self.request_ids.get(&key) {
            return match self.results.remove(&id) {
                Some(result) => {
                    self.request_ids.remove(&key);
                    Reply::Ready(serde_json::from_value(result).unwrap_or_default())
                }
                None => Reply::Pending,
            };
        }
        let sent = serde_json::to_value(params)
            .map_err(std::io::Error::from)
            .and_then(|params| self.connection.request(R::METHOD, params));
        if let Ok(id) = sent {
            self.request_ids.insert(key, id);
        }
        Reply::Pending
    }
}

/// `reply`'s answer through `convert`.
fn map_reply<T, U>(reply: Reply<T>, convert: impl FnOnce(T) -> U) -> Reply<U> {
    match reply {
        Reply::Ready(answer) => Reply::Ready(convert(answer)),
        Reply::Pending => Reply::Pending,
    }
}

fn completion_kind(kind: Option<lsp::CompletionItemKind>) -> CompletionKind {
    use lsp::CompletionItemKind as Kind;
    match kind.unwrap_or(Kind::TEXT) {
        Kind::METHOD => CompletionKind::Method,
        Kind::FUNCTION | Kind::CONSTRUCTOR => CompletionKind::Function,
        Kind::FIELD | Kind::PROPERTY => CompletionKind::Field,
        Kind::VARIABLE => CompletionKind::Variable,
        Kind::CLASS | Kind::INTERFACE | Kind::ENUM | Kind::STRUCT | Kind::TYPE_PARAMETER => CompletionKind::Type,
        Kind::MODULE => CompletionKind::Module,
        Kind::KEYWORD => CompletionKind::Keyword,
        Kind::SNIPPET => CompletionKind::Snippet,
        Kind::VALUE | Kind::ENUM_MEMBER | Kind::CONSTANT => CompletionKind::Constant,
        _ => CompletionKind::Text,
    }
}

fn completion_items(response: lsp::CompletionResponse) -> Vec<CompletionItem> {
    let items = match response {
        lsp::CompletionResponse::Array(items) => items,
        lsp::CompletionResponse::List(list) => list.items,
    };
    items
        .into_iter()
        .map(|item| {
            let insert_text = match item.text_edit {
                Some(lsp::CompletionTextEdit::Edit(edit)) => Some(edit.new_text),
                Some(lsp::CompletionTextEdit::InsertAndReplace(edit)) => Some(edit.new_text),
                None => item.insert_text,
            };
            CompletionItem {
                kind: completion_kind(item.kind),
                detail: item.detail,
                is_snippet: item.insert_text_format == Some(lsp::InsertTextFormat::SNIPPET),
                insert_text,
                label: item.label,
            }
        })
        .collect()
}

fn marked_string_text(marked: lsp::MarkedString) -> String {
    match marked {
        lsp::MarkedString::String(text) => text,
        lsp::MarkedString::LanguageString(code) => code.value,
    }
}

/// Hover contents as text; `None` when blank.
fn hover_text(contents: lsp::HoverContents) -> Option<String> {
    let text = match contents {
        lsp::HoverContents::Scalar(marked) => marked_string_text(marked),
        lsp::HoverContents::Array(parts) => parts.into_iter().map(marked_string_text).collect::<Vec<_>>().join("\n\n"),
        lsp::HoverContents::Markup(markup) => markup.value,
    };
    (!text.trim().is_empty()).then_some(text)
}

fn documentation_text(documentation: lsp::Documentation) -> String {
    match documentation {
        lsp::Documentation::String(text) => text,
        lsp::Documentation::MarkupContent(markup) => markup.value,
    }
}

/// The edits of a workspace edit that apply to `uri`.
fn edits_for_document(edit: lsp::WorkspaceEdit, uri: &Uri) -> Vec<lsp::TextEdit> {
    if let Some(edits) = edit.changes.and_then(|mut changes| changes.remove(uri)) {
        return edits;
    }
    let document_edits = match edit.document_changes {
        Some(lsp::DocumentChanges::Edits(edits)) => edits,
        Some(lsp::DocumentChanges::Operations(operations)) => operations
            .into_iter()
            .filter_map(|operation| match operation {
                lsp::DocumentChangeOperation::Edit(edit) => Some(edit),
                lsp::DocumentChangeOperation::Op(_) => None,
            })
            .collect(),
        None => Vec::new(),
    };
    document_edits
        .into_iter()
        .filter(|document_edit| document_edit.text_document.uri == *uri)
        .flat_map(|document_edit| document_edit.edits)
        .map(|edit| match edit {
            OneOf::Left(edit) => edit,
            OneOf::Right(annotated) => annotated.text_edit,
        })
        .collect()
}

fn symbol_kind(kind: lsp::SymbolKind) -> SymbolKind {
    use lsp::SymbolKind as Kind;
    match kind {
        Kind::FUNCTION => SymbolKind::Function,
        Kind::METHOD | Kind::CONSTRUCTOR => SymbolKind::Method,
        Kind::CLASS | Kind::INTERFACE | Kind::STRUCT | Kind::ENUM | Kind::TYPE_PARAMETER => SymbolKind::Type,
        Kind::MODULE | Kind::NAMESPACE | Kind::PACKAGE => SymbolKind::Module,
        Kind::CONSTANT | Kind::ENUM_MEMBER => SymbolKind::Constant,
        Kind::VARIABLE => SymbolKind::Variable,
        Kind::FIELD | Kind::PROPERTY => SymbolKind::Field,
        _ => SymbolKind::Other,
    }
}

/// Nested document symbols as a list, each child naming its parent as its container.
fn flattened_symbols(
    text: &Text,
    symbols: Vec<lsp::DocumentSymbol>,
    container: Option<&str>,
    flattened: &mut Vec<DocumentSymbol>,
) {
    for symbol in symbols {
        flattened.push(DocumentSymbol {
            name: symbol.name.clone(),
            kind: symbol_kind(symbol.kind),
            range: range_of(text, symbol.selection_range),
            container: container.map(str::to_string),
        });
        flattened_symbols(text, symbol.children.unwrap_or_default(), Some(&symbol.name), flattened);
    }
}

/// The active signature, with its active parameter as a char range of its label.
fn signature_help_of(help: lsp::SignatureHelp) -> Option<SignatureHelp> {
    let signature_index = help.active_signature.unwrap_or(0) as usize;
    let signature = help.signatures.into_iter().nth(signature_index)?;
    let label = signature.label;
    let parameter_index = signature.active_parameter.or(help.active_parameter).unwrap_or(0) as usize;
    let parameter = signature.parameters.unwrap_or_default().into_iter().nth(parameter_index);
    let active_parameter = parameter.and_then(|parameter| match parameter.label {
        lsp::ParameterLabel::Simple(parameter_label) => {
            let byte = label.find(&parameter_label)?;
            let start = label[..byte].chars().count();
            Some((start, start + parameter_label.chars().count()))
        }
        lsp::ParameterLabel::LabelOffsets([start, end]) => {
            Some((chars_in_utf16_prefix(&label, start as usize), chars_in_utf16_prefix(&label, end as usize)))
        }
    });
    let documentation = signature.documentation.map(documentation_text);
    Some(SignatureHelp { label, active_parameter, documentation })
}

impl LanguageServices for LspServices {
    fn completions(&mut self, text: &Text, text_revision: u64, position: Position) -> Reply<Vec<CompletionItem>> {
        let params = lsp::CompletionParams {
            text_document_position: self.text_document_position(text, position),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        };
        let reply = self.ask::<request::Completion>(
            text,
            RequestKey::new::<request::Completion>(text_revision, position),
            params,
        );
        map_reply(reply, |response| response.map(completion_items).unwrap_or_default())
    }

    fn take_diagnostics(&mut self, text: &Text) -> Option<Vec<Diagnostic>> {
        self.receive();
        let published = self.published_diagnostics.take()?;
        let diagnostics = published
            .into_iter()
            .map(|diagnostic| {
                let severity = match diagnostic.severity {
                    Some(lsp::DiagnosticSeverity::WARNING) => Severity::Warning,
                    Some(lsp::DiagnosticSeverity::INFORMATION) => Severity::Info,
                    Some(lsp::DiagnosticSeverity::HINT) => Severity::Hint,
                    _ => Severity::Error,
                };
                Diagnostic { range: range_of(text, diagnostic.range), severity, message: diagnostic.message }
            })
            .collect();
        Some(diagnostics)
    }

    fn rename(&mut self, text: &Text, text_revision: u64, position: Position, new_name: &str) -> Reply<Vec<TextEdit>> {
        let key = RequestKey::new::<request::Rename>(text_revision, position).with_argument(new_name);
        let params = lsp::RenameParams {
            text_document_position: self.text_document_position(text, position),
            new_name: new_name.to_string(),
            work_done_progress_params: Default::default(),
        };
        let uri = self.document_uri.clone();
        map_reply(self.ask::<request::Rename>(text, key, params), |edit| {
            edits_for_document(edit.unwrap_or_default(), &uri)
                .into_iter()
                .map(|edit| TextEdit { range: range_of(text, edit.range), new_text: edit.new_text })
                .collect()
        })
    }

    fn signature_help(&mut self, text: &Text, text_revision: u64, position: Position) -> Reply<Option<SignatureHelp>> {
        let params = lsp::SignatureHelpParams {
            context: None,
            text_document_position_params: self.text_document_position(text, position),
            work_done_progress_params: Default::default(),
        };
        let key = RequestKey::new::<request::SignatureHelpRequest>(text_revision, position);
        map_reply(self.ask::<request::SignatureHelpRequest>(text, key, params), |help| help.and_then(signature_help_of))
    }

    /// Sends the edits as ranges (in reverse order, so each is relative to the text after the
    /// previous one) when the server takes them; otherwise the next request syncs in full.
    fn text_changed(&mut self, texts_before: &[Text], changes: &[ChangeSet], text_revision_after: u64) {
        if !self.syncs_incrementally || self.synced_revision.is_none() || texts_before.len() != changes.len() {
            return;
        }
        let content_changes = texts_before
            .iter()
            .zip(changes)
            .flat_map(|(before, change_set)| {
                change_set.changes().iter().rev().map(|change| lsp::TextDocumentContentChangeEvent {
                    range: Some(lsp::Range::new(
                        lsp_position_of_offset(before, change.start),
                        lsp_position_of_offset(before, change.end),
                    )),
                    range_length: None,
                    text: change.insert.clone(),
                })
            })
            .collect();
        self.document_version += 1;
        let params = lsp::DidChangeTextDocumentParams {
            text_document: lsp::VersionedTextDocumentIdentifier::new(self.document_uri.clone(), self.document_version),
            content_changes,
        };
        if self.notify::<notification::DidChangeTextDocument>(params).is_ok() {
            self.synced_revision = Some(text_revision_after);
        }
    }

    fn document_highlights(
        &mut self,
        text: &Text,
        text_revision: u64,
        position: Position,
    ) -> Reply<Option<Vec<Range>>> {
        if !self.gives_document_highlights {
            return Reply::Ready(None);
        }
        let params = lsp::DocumentHighlightParams {
            text_document_position_params: self.text_document_position(text, position),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        let key = RequestKey::new::<request::DocumentHighlightRequest>(text_revision, position);
        map_reply(self.ask::<request::DocumentHighlightRequest>(text, key, params), |highlights| {
            highlights
                .map(|highlights| highlights.into_iter().map(|highlight| range_of(text, highlight.range)).collect())
        })
    }

    fn references(&mut self, text: &Text, text_revision: u64, position: Position) -> Reply<Vec<Location>> {
        let params = lsp::ReferenceParams {
            text_document_position: self.text_document_position(text, position),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: lsp::ReferenceContext { include_declaration: true },
        };
        let key = RequestKey::new::<request::References>(text_revision, position);
        let uri = self.document_uri.clone();
        map_reply(self.ask::<request::References>(text, key, params), |locations| {
            let mut converter = LocationConverter::new(text, &uri);
            locations
                .unwrap_or_default()
                .into_iter()
                .map(|location| converter.location(location.uri, location.range))
                .collect()
        })
    }

    fn inlay_hints(&mut self, text: &Text, text_revision: u64, lines: std::ops::Range<usize>) -> Reply<Vec<InlayHint>> {
        if !self.gives_inlay_hints || lines.is_empty() {
            return Reply::Ready(Vec::new());
        }
        let key = RequestKey::new::<request::InlayHintRequest>(text_revision, Position::new(lines.start, 0))
            .with_argument(lines.end.to_string());
        let end = if lines.end < text.line_count() {
            lsp::Position::new(lines.end as u32, 0)
        } else {
            let last_line = text.last_line();
            lsp_position(text, Position::new(last_line, text.line_len(last_line)))
        };
        let params = lsp::InlayHintParams {
            work_done_progress_params: Default::default(),
            text_document: self.text_document(),
            range: lsp::Range::new(lsp::Position::new(lines.start as u32, 0), end),
        };
        map_reply(self.ask::<request::InlayHintRequest>(text, key, params), |hints| {
            hints
                .unwrap_or_default()
                .into_iter()
                .filter(|hint| (hint.position.line as usize) < text.line_count())
                .map(|hint| {
                    let label = match hint.label {
                        lsp::InlayHintLabel::String(label) => label,
                        lsp::InlayHintLabel::LabelParts(parts) => parts.into_iter().map(|part| part.value).collect(),
                    };
                    let padding = |is_padded: Option<bool>| if is_padded == Some(true) { " " } else { "" };
                    let label = format!("{}{label}{}", padding(hint.padding_left), padding(hint.padding_right));
                    InlayHint { position: position_of(text, hint.position), label }
                })
                .collect()
        })
    }

    /// Code actions with edits to this document (commands to run on the server are left out).
    fn code_actions(&mut self, text: &Text, text_revision: u64, range: Range) -> Reply<Vec<CodeAction>> {
        let params = lsp::CodeActionParams {
            text_document: self.text_document(),
            range: lsp::Range::new(lsp_position(text, range.start), lsp_position(text, range.end)),
            context: lsp::CodeActionContext {
                trigger_kind: Some(lsp::CodeActionTriggerKind::INVOKED),
                ..Default::default()
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        let key = RequestKey::new::<request::CodeActionRequest>(text_revision, range.start)
            .with_argument(format!("{:?}", range.end));
        let uri = self.document_uri.clone();
        map_reply(self.ask::<request::CodeActionRequest>(text, key, params), |actions| {
            let mut code_actions: Vec<CodeAction> = actions
                .unwrap_or_default()
                .into_iter()
                .filter_map(|action| match action {
                    lsp::CodeActionOrCommand::CodeAction(action) => Some(action),
                    lsp::CodeActionOrCommand::Command(_) => None,
                })
                .filter_map(|action| {
                    let edits: Vec<TextEdit> = edits_for_document(action.edit?, &uri)
                        .into_iter()
                        .map(|edit| TextEdit { range: range_of(text, edit.range), new_text: edit.new_text })
                        .collect();
                    (!edits.is_empty()).then(|| CodeAction {
                        title: action.title,
                        is_quick_fix: action.kind.is_some_and(|kind| kind.as_str().starts_with("quickfix")),
                        is_preferred: action.is_preferred == Some(true),
                        edits,
                    })
                })
                .collect();
            code_actions.sort_by_key(|action| (!action.is_preferred, !action.is_quick_fix));
            code_actions
        })
    }

    fn format_document(
        &mut self,
        text: &Text,
        text_revision: u64,
        indentation: rustext_core::indentation::Indentation,
    ) -> Reply<Vec<TextEdit>> {
        let params = lsp::DocumentFormattingParams {
            text_document: self.text_document(),
            options: lsp::FormattingOptions {
                tab_size: indentation.tab_size as u32,
                insert_spaces: indentation.insert_spaces,
                ..Default::default()
            },
            work_done_progress_params: Default::default(),
        };
        let key = RequestKey::new::<request::Formatting>(text_revision, Position::new(0, 0));
        map_reply(self.ask::<request::Formatting>(text, key, params), |edits| {
            edits
                .unwrap_or_default()
                .into_iter()
                .map(|edit| TextEdit { range: range_of(text, edit.range), new_text: edit.new_text })
                .collect()
        })
    }

    fn document_symbols(&mut self, text: &Text, text_revision: u64) -> Reply<Vec<DocumentSymbol>> {
        let params = lsp::DocumentSymbolParams {
            text_document: self.text_document(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        let key = RequestKey::new::<request::DocumentSymbolRequest>(text_revision, Position::new(0, 0));
        let uri = self.document_uri.clone();
        map_reply(self.ask::<request::DocumentSymbolRequest>(text, key, params), |response| {
            let mut symbols = Vec::new();
            match response {
                Some(lsp::DocumentSymbolResponse::Nested(nested)) => {
                    flattened_symbols(text, nested, None, &mut symbols)
                }
                Some(lsp::DocumentSymbolResponse::Flat(flat)) => {
                    symbols.extend(flat.into_iter().filter(|symbol| symbol.location.uri == uri).map(|symbol| {
                        DocumentSymbol {
                            name: symbol.name,
                            kind: symbol_kind(symbol.kind),
                            range: range_of(text, symbol.location.range),
                            container: symbol.container_name,
                        }
                    }));
                }
                None => {}
            }
            symbols.sort_by_key(|symbol| symbol.range.start);
            symbols
        })
    }

    fn completion_trigger_chars(&self) -> &[char] {
        &self.trigger_chars
    }

    fn hover(&mut self, text: &Text, text_revision: u64, position: Position) -> Reply<Option<String>> {
        let params = lsp::HoverParams {
            text_document_position_params: self.text_document_position(text, position),
            work_done_progress_params: Default::default(),
        };
        let key = RequestKey::new::<request::HoverRequest>(text_revision, position);
        map_reply(self.ask::<request::HoverRequest>(text, key, params), |hover| {
            hover.and_then(|hover| hover_text(hover.contents))
        })
    }

    fn definition(&mut self, text: &Text, text_revision: u64, position: Position) -> Reply<Option<Location>> {
        let params = lsp::GotoDefinitionParams {
            text_document_position_params: self.text_document_position(text, position),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        let key = RequestKey::new::<request::GotoDefinition>(text_revision, position);
        let uri = self.document_uri.clone();
        map_reply(self.ask::<request::GotoDefinition>(text, key, params), |response| {
            let targets: Vec<(Uri, lsp::Range)> = match response? {
                lsp::GotoDefinitionResponse::Scalar(location) => vec![(location.uri, location.range)],
                lsp::GotoDefinitionResponse::Array(locations) => {
                    locations.into_iter().map(|location| (location.uri, location.range)).collect()
                }
                lsp::GotoDefinitionResponse::Link(links) => {
                    links.into_iter().map(|link| (link.target_uri, link.target_selection_range)).collect()
                }
            };
            let (target_uri, range) = targets.into_iter().next()?;
            Some(LocationConverter::new(text, &uri).location(target_uri, range))
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn columns_convert_between_chars_and_utf16() {
        let text = Text::new("a😀b");

        assert_eq!(utf16_column(&text, 0, 2), 3);
        assert_eq!(char_column(&text, 0, 3), 2);
    }

    #[test]
    fn file_uris_percent_encode_spaces() {
        assert_eq!(file_uri(std::path::Path::new("/my project/main.rs")), "file:///my%20project/main.rs");
    }

    #[test]
    fn file_uris_decode_back_to_their_path() {
        let path = std::path::Path::new("/my project/é.rs");

        assert_eq!(file_path_of(&file_uri(path)).as_deref(), Some(path));
        assert_eq!(file_path_of("https://example.com/a.rs"), None);
    }

    #[test]
    fn reads_completion_lists_and_arrays() {
        let list: lsp::CompletionResponse = serde_json::from_value(
            json!({ "isIncomplete": false, "items": [{ "label": "push", "kind": 2, "detail": "fn(&mut self, T)" }] }),
        )
        .unwrap();
        let array: lsp::CompletionResponse = serde_json::from_value(
            json!([{ "label": "fn", "kind": 15, "insertText": "fn ${1:name}() {}", "insertTextFormat": 2 }]),
        )
        .unwrap();

        assert_eq!(completion_items(list)[0].kind, CompletionKind::Method);
        let snippet = &completion_items(array)[0];
        assert!(snippet.is_snippet);
        assert_eq!(snippet.text_to_insert(), "fn ${1:name}() {}");
    }

    #[test]
    fn reads_hover_contents_in_every_shape() {
        let hover = |contents: Value| hover_text(serde_json::from_value(contents).unwrap());

        assert_eq!(hover(json!("plain")), Some("plain".into()));
        assert_eq!(hover(json!({ "kind": "markdown", "value": "**bold**" })), Some("**bold**".into()));
        assert_eq!(hover(json!([{ "language": "rust", "value": "fn a()" }, "doc"])), Some("fn a()\n\ndoc".into()));
        assert_eq!(hover(json!("  ")), None);
    }
}
