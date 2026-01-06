use super::expression::*;

#[derive(PartialEq, Clone, Debug)]
pub struct Array {
    pub elements: Vec<Expression>,
}

impl Array {
    pub fn new() -> Array {
        Array {
            elements: Vec::new(),
        }
    }

    pub fn count(&self) -> usize {
        self.elements.len()
    }

    pub fn push(&mut self, element: Expression) {
        self.elements.push(element);
    }
}

