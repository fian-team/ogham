use super::span::Span;

#[derive(PartialEq, Clone, Debug)]
pub struct Identifier {
    name: Box<String>,
    pub span: Span,
}

impl Identifier {
    pub fn new(identifier: &str, span: Span) -> Identifier {
        Identifier {
            name: Box::new(identifier.to_owned()),
            span,
        }
    }

    /// Create an identifier with a zero span, for synthetic/generated nodes.
    pub fn synthetic(identifier: &str) -> Identifier {
        Identifier {
            name: Box::new(identifier.to_owned()),
            span: Span::zero(),
        }
    }

    pub fn get(&self) -> String {
        *self.name.clone()
    }

    /// The name without cloning it. `get` hands back an owned `String`,
    /// which is fine for the compiler's one-shot reads and wrong for a
    /// walk that touches every identifier in a document.
    pub fn as_str(&self) -> &str {
        &self.name
    }
}
