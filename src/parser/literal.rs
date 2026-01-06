use super::{array::*, call::*, function::*, identifier::*, map::*};

#[derive(PartialEq, Clone, Debug)]
pub enum Literal {
    Integer(i32),
    Float(f64),
    Boolean(bool),
    Identifier(Identifier),
    Call(Call),
    Function(Function),
    String(String),
    Map(Map),
    Array(Array),
}

impl Literal {
    pub fn new_integer(value: i32) -> Literal {
        Literal::Integer(value)
    }

    pub fn new_identifier(value: Identifier) -> Literal {
        Literal::Identifier(value)
    }

    pub fn new_call(value: Call) -> Literal {
        Literal::Call(value)
    }

    pub fn new_function(value: Function) -> Literal {
        Literal::Function(value)
    }
}
