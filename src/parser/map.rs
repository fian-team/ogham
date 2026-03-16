use super::{expression::*, identifier::*, span::Span};

#[derive(PartialEq, Clone, Debug)]
pub struct Map {
    pub properties: Vec<(Identifier, Expression)>,
    pub span: Span,
}

impl Map {
    pub fn new() -> Map {
        Map {
            properties: Vec::new(),
            span: Span::zero(),
        }
    }

    pub fn count(&self) -> usize {
        self.properties.len()
    }

    pub fn set(&mut self, identifier: Identifier, value: Expression) {
        self.properties.push((identifier, value));
    }
}
