use super::expression::*;

#[derive(PartialEq, Clone, Debug)]
pub struct Call {
    pub callee: Box<Expression>,
    pub arguments: Vec<Expression>,
}

impl Call {
    pub fn new(callee: Expression, arguments: Vec<Expression>) -> Call {
        Call {
            callee: Box::new(callee),
            arguments,
        }
    }
}
