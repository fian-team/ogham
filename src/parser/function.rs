use super::{block::*, identifier::*};

#[derive(PartialEq, Clone, Debug)]
pub struct Function {
    pub arguments: Vec<Identifier>,
    pub return_type: Identifier,
    pub body: Block,
}

impl Function {
    pub fn new() -> Function {
        Function {
            arguments: Vec::new(),
            return_type: Identifier::new("infer"),
            body: Block::new(),
        }
    }
}
