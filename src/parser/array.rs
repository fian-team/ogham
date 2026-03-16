use super::{expression::*, span::Span};

#[derive(PartialEq, Clone, Debug)]
pub struct Array {
    pub elements: Vec<Expression>,
    pub span: Span,
}

impl Array {
    pub fn new() -> Array {
        Array {
            elements: Vec::new(),
            span: Span::zero(),
        }
    }

    pub fn count(&self) -> usize {
        self.elements.len()
    }

    pub fn push(&mut self, element: Expression) {
        self.elements.push(element);
    }
}
