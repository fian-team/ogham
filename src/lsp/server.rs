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

        // 5. Phase 2 conditional-hook warning. Walk the AST for
        //    `on_mount` / `on_unmount` statements that appear
        //    inside `if`, `match`, or for-loop bodies — these
        //    register conditionally and don't behave as authors
        //    expect. Advisory only.
        for warning in collect_lifecycle_warnings(&module) {
            diagnostics.push(syntax_error_to_diagnostic(&warning));
        }
    }

    diagnostics
}

/// Walk an AST and emit warnings for `on_mount` / `on_unmount`
/// statements appearing inside `if` branches, `else` blocks, or
/// for-loop bodies. These register conditionally — at runtime
/// the hook only registers when its surrounding control-flow
/// path is taken. Per the design (decision #16), this is legal
/// at runtime but earns a warning at the LSP because the
/// semantic almost certainly isn't what the author wanted (use
/// `effect (cond) { ... }` for "run when the flag changes").
fn collect_lifecycle_warnings(
    module: &ogham::parser::Function,
) -> Vec<ogham::parser::SyntaxError> {
    let mut warnings = Vec::new();
    walk_block_for_hooks(&module.body, /* in_conditional */ false, &mut warnings);
    warnings
}

fn walk_block_for_hooks(
    block: &ogham::parser::Block,
    in_conditional: bool,
    out: &mut Vec<ogham::parser::SyntaxError>,
) {
    for stmt in &block.statement_list {
        walk_stmt_for_hooks(stmt, in_conditional, out);
    }
}

fn walk_stmt_for_hooks(
    stmt: &ogham::parser::Statement,
    in_conditional: bool,
    out: &mut Vec<ogham::parser::SyntaxError>,
) {
    use ogham::parser::Statement;
    match stmt {
        Statement::OnMount(hook) | Statement::OnUnmount(hook) => {
            let kind = match stmt {
                Statement::OnMount(_) => "on_mount",
                _ => "on_unmount",
            };
            if in_conditional {
                out.push(
                    ogham::parser::SyntaxError::new(
                        hook.span.start_line,
                        hook.span.start_column,
                        format!(
                            "{kind} inside a conditional fires only \
                             if its path is also newly-mounted that \
                             frame"
                        ),
                    )
                    .with_help(format!(
                        "for \"run when this flag changes\" use \
                         `effect (flag) {{ ... }}` instead"
                    ))
                    .with_warning(),
                );
            }
            // Recurse into the hook's body — nested hooks (e.g.
            // an `on_mount` whose body contains a closure that
            // declares another `on_mount`) follow the same rule.
            walk_block_for_hooks(&hook.body, in_conditional, out);
        }
        Statement::Effect(effect) => {
            if in_conditional {
                out.push(
                    ogham::parser::SyntaxError::new(
                        effect.span.start_line,
                        effect.span.start_column,
                        "effect inside a conditional won't be tracked \
                         when the condition is false",
                    )
                    .with_help(
                        "consider moving the effect to top-level and \
                         using `if` inside the body instead",
                    )
                    .with_warning(),
                );
            }
            // Recurse into the body. Effects don't reset the
            // conditional context — a cleanup inside an effect
            // inside an if is still inside a conditional.
            walk_block_for_hooks(&effect.body, in_conditional, out);
        }
        Statement::Cleanup(hook) => {
            walk_block_for_hooks(&hook.body, in_conditional, out);
        }
        Statement::Conditional(cond) => {
            for (_test, branch) in &cond.branches {
                walk_block_for_hooks(branch, /* now in conditional */ true, out);
            }
            if let Some(else_block) = &cond.else_block {
                walk_block_for_hooks(else_block, true, out);
            }
        }
        Statement::ForLoop(for_loop) => {
            walk_block_for_hooks(&for_loop.body, true, out);
        }
        // `let foo = fn () { ... }` — descend into the function
        // body so hooks declared inside (the common case) are
        // visited. Without this descent, the warning never
        // fires in real consumer code: hooks live in fn bodies,
        // not at module top level.
        Statement::Declare(d) => {
            if let ogham::parser::Expression::Literal(
                ogham::parser::Literal::Function(f),
            ) = &d.value
            {
                walk_block_for_hooks(&f.body, in_conditional, out);
            }
        }
        // Other statement kinds don't introduce conditional
        // contexts and don't contain blocks of statements.
        // Expression statements *can* contain blocks (match arms
        // produce expressions), but match-arm bodies live in
        // Expression, not Statement; lifecycle hooks are
        // statements, not expressions, so they can't appear
        // inside a match-arm body anyway.
        _ => {}
    }
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
    let severity = match err.severity {
        ogham::parser::DiagnosticLevel::Error => DiagnosticSeverity::ERROR,
        ogham::parser::DiagnosticLevel::Warning => DiagnosticSeverity::WARNING,
    };
    Diagnostic {
        range: Range::new(Position::new(line, col), Position::new(line, col + length)),
        severity: Some(severity),
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
    use super::{collect_lifecycle_warnings, format_diagnostic_message};

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

    #[test]
    fn lifecycle_warning_descends_into_fn_bodies() {
        // Regression for the audit finding: the walker used to
        // bail at Statement::Declare, so warnings on hooks
        // declared inside `let main = fn () { ... }` (the
        // common shape) never fired.
        let src = r#"
let main = fn () {
  if true {
    on_mount { log "x"; };
  }
};
"#;
        let mut scanner = ogham::scanner::Scanner::new(src.to_string());
        let mut parser = ogham::parser::Parser::new(scanner.scan());
        let module = parser.parse().expect("parse");
        let warnings = collect_lifecycle_warnings(&module);
        assert_eq!(warnings.len(), 1, "should warn on the conditional on_mount");
        assert!(
            warnings[0].message.contains("on_mount inside a conditional"),
            "warning text mismatch: {}",
            warnings[0].message
        );
        assert_eq!(
            warnings[0].severity,
            ogham::parser::DiagnosticLevel::Warning
        );
    }

    #[test]
    fn lifecycle_warning_fires_on_effect_inside_for_loop() {
        // Companion to the previous test — verifies effects also
        // get the warning, and that for-loops count as
        // conditional contexts.
        let src = r#"
let main = fn () {
  for (i in 0..3) {
    effect (i) { log "x"; };
  }
};
"#;
        let mut scanner = ogham::scanner::Scanner::new(src.to_string());
        let mut parser = ogham::parser::Parser::new(scanner.scan());
        let module = parser.parse().expect("parse");
        let warnings = collect_lifecycle_warnings(&module);
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].message.contains("effect inside a conditional"),
            "warning text mismatch: {}",
            warnings[0].message
        );
    }

    #[test]
    fn lifecycle_warning_silent_when_hook_is_top_level_in_fn() {
        // Hooks at the top of a fn body — the normal, correct
        // shape — must NOT warn.
        let src = r#"
let main = fn () {
  on_mount { log "ok"; };
  effect () { log "ok"; };
};
"#;
        let mut scanner = ogham::scanner::Scanner::new(src.to_string());
        let mut parser = ogham::parser::Parser::new(scanner.scan());
        let module = parser.parse().expect("parse");
        let warnings = collect_lifecycle_warnings(&module);
        assert!(
            warnings.is_empty(),
            "hooks at fn top-level should not warn; got {:?}",
            warnings
        );
    }
}
