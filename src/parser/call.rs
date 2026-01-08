use super::{expression::*, identifier::*};

#[derive(PartialEq, Clone, Debug)]
pub struct Call {
    pub identifier: Identifier,
    pub arguments: Vec<Expression>,
}

impl Call {
    pub fn new(identifier: Identifier, arguments: Vec<Expression>) -> Call {
        Call {
            identifier,
            arguments,
        }
    }
}
