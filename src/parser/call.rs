use super::{expression::*, span::Span};

#[derive(PartialEq, Clone, Debug)]
pub struct Call {
    pub callee: Box<Expression>,
    pub arguments: Vec<Expression>,
    pub span: Span,
}

impl Call {
    pub fn new(callee: Expression, arguments: Vec<Expression>, span: Span) -> Call {
        Call {
            callee: Box::new(callee),
            arguments,
            span,
        }
    }
}
