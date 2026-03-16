use super::{expression::*, identifier::*, span::Span};

#[derive(PartialEq, Clone, Debug)]
pub struct Widget {
    pub identifier: Identifier,
    pub properties: Vec<(Identifier, Expression)>,
    pub span: Span,
}

impl Widget {
    pub fn new(identifier: Identifier) -> Widget {
        Widget {
            identifier,
            properties: Vec::new(),
            span: Span::zero(),
        }
    }

    pub fn set(&mut self, key: Identifier, value: Expression) {
        self.properties.push((key, value));
    }
}
