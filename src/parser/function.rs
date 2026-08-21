use super::{block::*, identifier::*, span::Span};

#[derive(PartialEq, Clone, Debug)]
pub struct Function {
    pub arguments: Vec<Identifier>,
    pub return_type: Identifier,
    pub body: Block,
    pub span: Span,
    /// Every type annotation written anywhere in this module, in source
    /// order — a parameter's and a return's alike.
    ///
    /// Only the **module**'s function carries them, because the question
    /// they are collected for is a module-scope one: does this name
    /// resolve to a record this module declares or imports
    /// (`APPLICATION.md` §4.1 — an annotation nothing checks is a false
    /// expectation). A nested `fn`'s own list stays empty; the parser
    /// gathers as it goes and hands the whole list to the module it
    /// finished parsing.
    pub annotations: Vec<Identifier>,
}

impl Function {
    pub fn new() -> Function {
        Function {
            arguments: Vec::new(),
            return_type: Identifier::synthetic("infer"),
            body: Block::new(),
            span: Span::zero(),
            annotations: Vec::new(),
        }
    }
}
