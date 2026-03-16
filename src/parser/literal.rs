use super::{array::*, function::*, identifier::*, map::*, span::Span};

#[derive(PartialEq, Clone, Debug)]
pub enum Literal {
    Integer(i32, Span),
    Float(f64, Span),
    Boolean(bool, Span),
    Identifier(Identifier),
    Function(Function),
    String(String, Span),
    Map(Map),
    Array(Array),
}

impl Literal {
    pub fn new_integer(value: i32, span: Span) -> Literal {
        Literal::Integer(value, span)
    }

    pub fn new_identifier(value: Identifier) -> Literal {
        Literal::Identifier(value)
    }

    pub fn new_function(value: Function) -> Literal {
        Literal::Function(value)
    }

    pub fn span(&self) -> Span {
        match self {
            Literal::Integer(_, span) => *span,
            Literal::Float(_, span) => *span,
            Literal::Boolean(_, span) => *span,
            Literal::String(_, span) => *span,
            Literal::Identifier(ident) => ident.span,
            Literal::Function(func) => func.span,
            Literal::Map(map) => map.span,
            Literal::Array(array) => array.span,
        }
    }
}
