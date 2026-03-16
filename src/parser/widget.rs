use super::{expression::*, identifier::*, span::Span};
use std::collections::HashMap;

#[derive(PartialEq, Clone, Debug)]
pub struct Widget {
    pub identifier: Identifier,
    pub properties: HashMap<String, Expression>,
    pub span: Span,
}

impl Widget {
    pub fn new(identifier: Identifier) -> Widget {
        Widget {
            identifier,
            properties: HashMap::new(),
            span: Span::zero(),
        }
    }

    pub fn set(&mut self, key: Identifier, value: Expression) {
        self.properties.insert(key.get(), value);
    }
}
