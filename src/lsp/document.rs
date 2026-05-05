use ogham::parser::{Function, SyntaxError};
use ogham::runtime::schema::ModuleSchema;
use ogham::scanner::Token;
use std::collections::HashMap;
use tower_lsp::lsp_types::Url;

/// Cached analysis results for a single open document.
pub struct Document {
    pub source: String,
    pub tokens: Vec<Token>,
    pub ast: Option<Function>,
    /// The module's resolved schema, when both parsing and the
    /// schema resolver succeed. `None` for parse-failed or
    /// schema-rejected documents (the latter case also produces
    /// an entry in `schema_error`).
    pub schema: Option<ModuleSchema>,
    /// Schema-resolution error surfaced by the LSP as a diagnostic.
    /// Stored alongside the schema so hover/completion can fall
    /// back when present.
    pub schema_error: Option<SyntaxError>,
}

impl Document {
    pub fn new(source: String) -> Self {
        Self {
            source,
            tokens: Vec::new(),
            ast: None,
            schema: None,
            schema_error: None,
        }
    }

    /// Re-scan, re-parse, and re-resolve the current source text.
    /// Cheap to call on every keystroke (nothing is cached
    /// across calls).
    pub fn analyze(&mut self) {
        let mut scanner = ogham::scanner::Scanner::new(self.source.clone());
        self.tokens = scanner.scan();
        let mut parser = ogham::parser::Parser::new(self.tokens.clone());
        self.ast = parser.parse().ok();
        // Schema resolution is best-effort: only attempt it when
        // the parser produced an AST. Failures surface as a
        // diagnostic via `schema_error`.
        self.schema = None;
        self.schema_error = None;
        if let Some(ast) = &self.ast {
            match ModuleSchema::from_module(ast) {
                Ok(schema) => self.schema = Some(schema),
                Err(err) => self.schema_error = Some(err),
            }
        }
    }
}

/// Stores all open documents keyed by their URI.
pub struct DocumentStore {
    documents: HashMap<Url, Document>,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
        }
    }

    pub fn open(&mut self, uri: Url, source: String) -> &mut Document {
        let mut doc = Document::new(source);
        doc.analyze();
        self.documents.entry(uri).insert_entry(doc).into_mut()
    }

    pub fn get(&self, uri: &Url) -> Option<&Document> {
        self.documents.get(uri)
    }

    pub fn get_mut(&mut self, uri: &Url) -> Option<&mut Document> {
        self.documents.get_mut(uri)
    }

    pub fn close(&mut self, uri: &Url) {
        self.documents.remove(uri);
    }
}
