use super::{literal::*, operator::*, widget::*};

#[derive(PartialEq, Clone, Debug)]
pub enum Expression {
    Literal(Literal),   // 5 1 3 true "Hello world!"
    Unary(Unary),       // -5 !true
    Binary(Binary),     // 5 + 3, 8 * 1
    Grouping(Grouping), // (5 + 3)
    Widget(Widget),     // WidgetIdentifier { key: value }
}

impl Expression {
    pub fn new_unary(value: Expression) -> Expression {
        Expression::Unary(Unary::new(value))
    }
    pub fn new_binary(left: Expression, operator: Operator, right: Expression) -> Expression {
        Expression::Binary(Binary::new(left, operator, right))
    }
    pub fn new_literal(literal: Literal) -> Expression {
        Expression::Literal(literal)
    }
    pub fn new_widget(widget: Widget) -> Expression {
        Expression::Widget(widget)
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct Grouping {
    pub value: Box<Expression>,
}

#[derive(PartialEq, Clone, Debug)]
pub struct Unary {
    pub value: Box<Expression>,
}

impl Unary {
    pub fn new(value: Expression) -> Unary {
        Unary {
            value: Box::new(value),
        }
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct Binary {
    pub left: Box<Expression>,
    pub right: Box<Expression>,
    pub operator: Operator,
}

impl Binary {
    pub fn new(left: Expression, operator: Operator, right: Expression) -> Binary {
        Binary {
            left: Box::new(left),
            right: Box::new(right),
            operator,
        }
    }
}
