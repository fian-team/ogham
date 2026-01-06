use super::{block::*, call::*, expression::*};

#[derive(PartialEq, Clone, Debug)]
pub enum Node {
    Block(Block),
    Text(String),
    Call(Call),
    Expression(Expression),
}
