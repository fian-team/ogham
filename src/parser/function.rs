use super::{block::*, identifier::*, span::Span};

#[derive(PartialEq, Clone, Debug)]
pub struct Function {
    pub arguments: Vec<Identifier>,
    pub return_type: Identifier,
    pub body: Block,
    pub span: Span,
}

impl Function {
    pub fn new() -> Function {
        Function {
            arguments: Vec::new(),
            return_type: Identifier::synthetic("infer"),
            body: Block::new(),
            span: Span::zero(),
        }
    }
}
