use std::sync::Mutex;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::document::DocumentStore;
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
        let Some(info) = hover::hover_at(ast, line, col) else {
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

/// Collect diagnostics from a document's scan+parse results.
fn collect_diagnostics(doc: &crate::document::Document) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

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

    let mut parser = ogham::parser::Parser::new(doc.tokens.clone());
    if let Err(err) = parser.parse() {
        let line = err.line.saturating_sub(1) as u32;
        let col = err.column.saturating_sub(1) as u32;
        diagnostics.push(Diagnostic {
            range: Range::new(Position::new(line, col), Position::new(line, col + 1)),
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("ogham".to_string()),
            message: err.message,
            ..Default::default()
        });
    }

    diagnostics
}
