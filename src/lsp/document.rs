use ogham::parser::Function;
use ogham::scanner::Token;
use std::collections::HashMap;
use tower_lsp::lsp_types::Url;

/// Cached analysis results for a single open document.
pub struct Document {
    pub source: String,
    pub tokens: Vec<Token>,
    pub ast: Option<Function>,
}

impl Document {
    pub fn new(source: String) -> Self {
        Self {
            source,
            tokens: Vec::new(),
            ast: None,
        }
    }

    /// Re-scan and re-parse the current source text.
    pub fn analyze(&mut self) {
        let mut scanner = ogham::scanner::Scanner::new(self.source.clone());
        self.tokens = scanner.scan();
        let mut parser = ogham::parser::Parser::new(self.tokens.clone());
        self.ast = parser.parse().ok();
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
