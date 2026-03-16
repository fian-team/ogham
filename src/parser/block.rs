use super::{span::Span, statement::*};

#[derive(PartialEq, Clone, Debug)]
pub struct Block {
    pub statement_list: Vec<Statement>,
    pub span: Span,
}

impl Block {
    pub fn new() -> Block {
        Block {
            statement_list: Vec::new(),
            span: Span::zero(),
        }
    }
}
