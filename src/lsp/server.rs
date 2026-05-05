use std::sync::Mutex;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::document::DocumentStore;
use crate::document_symbols;
use crate::goto_definition;
use crate::hover;
use crate::semantic_tokens;

pub struct OghamLanguageServer {
    client: Client,
    store: Mutex<DocumentStore>,
}

impl OghamLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            store: Mutex::new(DocumentStore::new()),
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for OghamLanguageServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: SemanticTokensLegend {
                                token_types: semantic_tokens::TOKEN_TYPES.to_vec(),
                                token_modifiers: semantic_tokens::TOKEN_MODIFIERS.to_vec(),
                            },
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: None,
                            ..Default::default()
                        },
                    ),
                ),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "ogham-lsp initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let diagnostics = {
            let mut store = self.store.lock().unwrap();
            let doc = store.open(uri.clone(), params.text_document.text);
            collect_diagnostics(doc)
        };
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let diagnostics = {
            let mut store = self.store.lock().unwrap();
            if let Some(doc) = store.get_mut(&uri) {
                apply_edits(&mut doc.source, &params.content_changes);
                doc.analyze();
                collect_diagnostics(doc)
            } else {
                vec![]
            }
        };
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        self.store.lock().unwrap().close(&uri);
        self.client
            .publish_diagnostics(uri, vec![], None)
            .await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let store = self.store.lock().unwrap();
        let Some(doc) = store.get(uri) else {
            return Ok(None);
        };
        let Some(ast) = &doc.ast else {
            return Ok(None);
        };
        // LSP positions are 0-indexed; our spans are 1-indexed.
        let line = pos.line as usize + 1;
        let col = pos.character as usize + 1;
        let Some(info) = hover::hover_at(ast, line, col, doc.schema.as_ref()) else {
            return Ok(None);
        };
        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: info.to_markdown(),
            }),
            range: None,
        }))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri.clone();
        let pos = params.text_document_position_params.position;
        let store = self.store.lock().unwrap();
        let Some(doc) = store.get(&uri) else {
            return Ok(None);
        };
        let Some(ast) = &doc.ast else {
            return Ok(None);
        };
        let line = pos.line as usize + 1;
        let col = pos.character as usize + 1;
        let Some(span) = goto_definition::definition_at(ast, line, col) else {
            return Ok(None);
        };
        Ok(Some(GotoDefinitionResponse::Scalar(Location {
            uri,
            range: goto_definition::span_to_range(&span),
        })))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let store = self.store.lock().unwrap();
        let Some(doc) = store.get(&params.text_document.uri) else {
            return Ok(None);
        };
        let Some(ast) = &doc.ast else {
            return Ok(None);
        };
        let symbols = document_symbols::document_symbols(ast);
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let store = self.store.lock().unwrap();
        let Some(doc) = store.get(&params.text_document.uri) else {
            return Ok(None);
        };
        let tokens =
            semantic_tokens::build_semantic_tokens(&doc.tokens, doc.ast.as_ref());
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: tokens,
        })))
    }
}

/// Apply incremental text edits to the stored source string.
fn apply_edits(source: &mut String, changes: &[TextDocumentContentChangeEvent]) {
    for change in changes {
        if let Some(range) = change.range {
            let start_offset = position_to_offset(source, range.start);
            let end_offset = position_to_offset(source, range.end);
            source.replace_range(start_offset..end_offset, &change.text);
        } else {
            *source = change.text.clone();
        }
    }
}

/// Convert an LSP Position (0-indexed line/character) to a byte offset.
fn position_to_offset(text: &str, pos: Position) -> usize {
    let mut offset = 0;
    for (i, line) in text.split('\n').enumerate() {
        if i == pos.line as usize {
            return offset + byte_offset_of_utf16_cu(line, pos.character as usize);
        }
        offset += line.len() + 1; // +1 for the '\n'
    }
    text.len()
}

/// Convert a UTF-16 code-unit offset within a line to a byte offset.
fn byte_offset_of_utf16_cu(line: &str, utf16_offset: usize) -> usize {
    let mut utf16_count = 0;
    for (byte_idx, ch) in line.char_indices() {
        if utf16_count >= utf16_offset {
            return byte_idx;
        }
        utf16_count += ch.len_utf16();
    }
    line.len()
}

/// Collect diagnostics from a document's scan+parse+schema+compile
/// results. The pipeline cascades — once a stage errors, later
/// stages skip — to avoid drowning the editor in derivative noise.
fn collect_diagnostics(doc: &crate::document::Document) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // 1. Scanner errors (Error tokens).
    for token in &doc.tokens {
        if let ogham::scanner::TokenType::Error(msg) = &token.token_type {
            let line = token.line.saturating_sub(1) as u32;
            let col = token.column.saturating_sub(1) as u32;
            diagnostics.push(Diagnostic {
                range: Range::new(Position::new(line, col), Position::new(line, col + token.length as u32)),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("ogham".to_string()),
                message: msg.clone(),
                ..Default::default()
            });
        }
    }

    // 2. Parser errors. Re-run the parser here (cheap, gives us
    //    the typed Err shape rather than relying on the cached
    //    Option<Function> in the Document).
    let mut parser = ogham::parser::Parser::new(doc.tokens.clone());
    let parsed = parser.parse();
    if let Err(err) = &parsed {
        diagnostics.push(syntax_error_to_diagnostic(err));
        return diagnostics; // schema/compile checks need a successful parse
    }

    // 3. Schema resolver errors (already cached on the Document
    //    for hover/completion to use, but we render here too).
    if let Some(err) = &doc.schema_error {
        diagnostics.push(syntax_error_to_diagnostic(err));
        return diagnostics; // compile check needs a successful schema
    }

    // 4. Strict-mode compile errors. The compiler builds its own
    //    schema internally; that's fine — duplicate work but
    //    quick. Surface only StrictMode errors here; other VMError
    //    variants (e.g. UndefinedVariable in loose mode) aren't
    //    LSP-actionable.
    if let Ok(module) = parsed {
        if let Err(ogham::runtime::error::VMError::StrictMode(err)) =
            ogham::runtime::compiler::Compiler::compile_module(&module)
        {
            diagnostics.push(syntax_error_to_diagnostic(&err));
        }
    }

    diagnostics
}

/// Convert a [`SyntaxError`] into an LSP [`Diagnostic`], rendering
/// the optional `note:` / `help:` lines into the message field.
fn syntax_error_to_diagnostic(err: &ogham::parser::SyntaxError) -> Diagnostic {
    let line = err.line.saturating_sub(1) as u32;
    let col = err.column.saturating_sub(1) as u32;
    // length 0 falls back to a one-character highlight, matching the
    // pre-typed-bindings behavior. Strict-mode errors set length
    // explicitly via `SyntaxError::with_length`.
    let length = if err.length > 0 { err.length as u32 } else { 1 };
    Diagnostic {
        range: Range::new(Position::new(line, col), Position::new(line, col + length)),
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("ogham".to_string()),
        message: format_diagnostic_message(
            &err.message,
            err.note.as_deref(),
            err.help.as_deref(),
        ),
        ..Default::default()
    }
}

/// Render a parser/scanner diagnostic into the multi-line shape clients
/// see in their problems pane: the primary message on the first line,
/// then optional `note:` and `help:` continuation lines (Rust's compiler
/// convention). Stuffing them into the message field is the v1 strategy
/// — `tower_lsp::Diagnostic::related_information` is fancier but
/// requires a `Location` that strict-mode errors don't always have a
/// natural value for.
fn format_diagnostic_message(message: &str, note: Option<&str>, help: Option<&str>) -> String {
    match (note, help) {
        (None, None) => message.to_string(),
        (Some(n), None) => format!("{message}\n\nnote: {n}"),
        (None, Some(h)) => format!("{message}\n\nhelp: {h}"),
        (Some(n), Some(h)) => format!("{message}\n\nnote: {n}\nhelp: {h}"),
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::format_diagnostic_message;

    #[test]
    fn formats_plain_message_unchanged() {
        assert_eq!(format_diagnostic_message("oops", None, None), "oops");
    }

    #[test]
    fn formats_message_with_note() {
        assert_eq!(
            format_diagnostic_message("oops", Some("rule explanation"), None),
            "oops\n\nnote: rule explanation"
        );
    }

    #[test]
    fn formats_message_with_help() {
        assert_eq!(
            format_diagnostic_message("oops", None, Some("did you mean `foo`?")),
            "oops\n\nhelp: did you mean `foo`?"
        );
    }

    #[test]
    fn formats_message_with_note_and_help() {
        assert_eq!(
            format_diagnostic_message("oops", Some("rule"), Some("fix")),
            "oops\n\nnote: rule\nhelp: fix"
        );
    }
}
