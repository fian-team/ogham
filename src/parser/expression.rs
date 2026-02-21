use super::{block::*, call::*, identifier::*, literal::*, operator::*, widget::*};

#[derive(PartialEq, Clone, Debug)]
pub enum Expression {
    Literal(Literal),                 // 5 1 3 true "Hello world!"
    Unary(Unary),                     // -5 !true
    Binary(Binary),                   // 5 + 3, 8 * 1
    Grouping(Grouping),               // (5 + 3)
    Widget(Widget),                   // WidgetIdentifier { key: value }
    MemberAccess(MemberAccess),       // foo.bar
    Call(Call),                       // foo() or array.length()
    IndexAccess(IndexAccess),         // array[index]
    Range(RangeExpression),           // 0..5
    ForLoop(ForLoopExpression),       // for (i in 0..5) { ... }
    SpreadForLoop(ForLoopExpression), // ...for (i in 0..5) { ... }
    Spread(Box<Expression>),          // ...expr (e.g. in array literals)
    Match(MatchExpression),           // match expr { pat => body, ... }
}

impl Expression {
    pub fn new_unary(operator: Operator, value: Expression) -> Expression {
        Expression::Unary(Unary::new(operator, value))
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

    pub fn new_member_access(object: Expression, property: Identifier) -> Expression {
        Expression::MemberAccess(MemberAccess::new(object, property))
    }

    pub fn new_call(callee: Expression, arguments: Vec<Expression>) -> Expression {
        Expression::Call(Call::new(callee, arguments))
    }

    pub fn new_index_access(object: Expression, index: Expression) -> Expression {
        Expression::IndexAccess(IndexAccess::new(object, index))
    }

    pub fn new_range(start: Expression, end: Expression) -> Expression {
        Expression::Range(RangeExpression::new(start, end))
    }

    pub fn new_for_loop(
        variable: Identifier,
        range_start: Expression,
        range_end: Expression,
        body: Block,
    ) -> Expression {
        Expression::ForLoop(ForLoopExpression::new(
            variable,
            range_start,
            range_end,
            body,
        ))
    }

    pub fn new_spread_for_loop(
        variable: Identifier,
        range_start: Expression,
        range_end: Expression,
        body: Block,
    ) -> Expression {
        Expression::SpreadForLoop(ForLoopExpression::new(
            variable,
            range_start,
            range_end,
            body,
        ))
    }

    pub fn new_spread(expr: Expression) -> Expression {
        Expression::Spread(Box::new(expr))
    }

    pub fn new_match(scrutinee: Expression, arms: Vec<(Expression, Block)>) -> Expression {
        Expression::Match(MatchExpression::new(scrutinee, arms))
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct IndexAccess {
    pub object: Box<Expression>,
    pub index: Box<Expression>,
}

impl IndexAccess {
    pub fn new(object: Expression, index: Expression) -> Self {
        Self {
            object: Box::new(object),
            index: Box::new(index),
        }
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct MemberAccess {
    pub object: Box<Expression>,
    pub property: Identifier,
}

impl MemberAccess {
    pub fn new(object: Expression, property: Identifier) -> Self {
        Self {
            object: Box::new(object),
            property,
        }
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct Grouping {
    pub value: Box<Expression>,
}

#[derive(PartialEq, Clone, Debug)]
pub struct Unary {
    pub operator: Operator,
    pub value: Box<Expression>,
}

impl Unary {
    pub fn new(operator: Operator, value: Expression) -> Unary {
        Unary {
            operator,
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

#[derive(PartialEq, Clone, Debug)]
pub struct RangeExpression {
    pub start: Box<Expression>,
    pub end: Box<Expression>,
}

impl RangeExpression {
    pub fn new(start: Expression, end: Expression) -> RangeExpression {
        RangeExpression {
            start: Box::new(start),
            end: Box::new(end),
        }
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct ForLoopExpression {
    pub variable: Identifier,
    pub range_start: Box<Expression>,
    pub range_end: Box<Expression>,
    pub body: Block,
}

impl ForLoopExpression {
    pub fn new(
        variable: Identifier,
        range_start: Expression,
        range_end: Expression,
        body: Block,
    ) -> ForLoopExpression {
        ForLoopExpression {
            variable,
            range_start: Box::new(range_start),
            range_end: Box::new(range_end),
            body,
        }
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct MatchExpression {
    pub scrutinee: Box<Expression>,
    pub arms: Vec<(Expression, Block)>,
}

impl MatchExpression {
    pub fn new(scrutinee: Expression, arms: Vec<(Expression, Block)>) -> MatchExpression {
        MatchExpression {
            scrutinee: Box::new(scrutinee),
            arms,
        }
    }
}
