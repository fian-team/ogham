use super::{block::*, call::*, identifier::*, literal::*, operator::*, span::Span, widget::*};

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
    Spread(SpreadExpression),         // ...expr (e.g. in array literals)
    Match(MatchExpression),           // match expr { pat => body, ... }
    PrefixIncrement(IncrementExpression),  // ++x
    PostfixIncrement(IncrementExpression), // x++
}

impl Expression {
    pub fn new_unary(operator: Operator, value: Expression, span: Span) -> Expression {
        Expression::Unary(Unary::new(operator, value, span))
    }
    pub fn new_binary(
        left: Expression,
        operator: Operator,
        right: Expression,
        span: Span,
    ) -> Expression {
        Expression::Binary(Binary::new(left, operator, right, span))
    }
    pub fn new_literal(literal: Literal) -> Expression {
        Expression::Literal(literal)
    }
    pub fn new_widget(widget: Widget) -> Expression {
        Expression::Widget(widget)
    }

    pub fn new_member_access(object: Expression, property: Identifier, span: Span) -> Expression {
        Expression::MemberAccess(MemberAccess::new(object, property, span))
    }

    pub fn new_call(callee: Expression, arguments: Vec<Expression>, span: Span) -> Expression {
        Expression::Call(Call::new(callee, arguments, span))
    }

    pub fn new_index_access(object: Expression, index: Expression, span: Span) -> Expression {
        Expression::IndexAccess(IndexAccess::new(object, index, span))
    }

    pub fn new_range(start: Expression, end: Expression, span: Span) -> Expression {
        Expression::Range(RangeExpression::new(start, end, span))
    }

    pub fn new_for_loop(
        variable: Identifier,
        range_start: Expression,
        range_end: Expression,
        body: Block,
        span: Span,
    ) -> Expression {
        Expression::ForLoop(ForLoopExpression::new(
            variable,
            range_start,
            range_end,
            body,
            span,
        ))
    }

    pub fn new_spread_for_loop(
        variable: Identifier,
        range_start: Expression,
        range_end: Expression,
        body: Block,
        span: Span,
    ) -> Expression {
        Expression::SpreadForLoop(ForLoopExpression::new(
            variable,
            range_start,
            range_end,
            body,
            span,
        ))
    }

    pub fn new_spread(expr: Expression, span: Span) -> Expression {
        Expression::Spread(SpreadExpression {
            inner: Box::new(expr),
            span,
        })
    }

    pub fn new_match(
        scrutinee: Expression,
        arms: Vec<(Expression, Block)>,
        span: Span,
    ) -> Expression {
        Expression::Match(MatchExpression::new(scrutinee, arms, span))
    }

    pub fn span(&self) -> Span {
        match self {
            Expression::Literal(lit) => lit.span(),
            Expression::Unary(u) => u.span,
            Expression::Binary(b) => b.span,
            Expression::Grouping(g) => g.span,
            Expression::Widget(w) => w.span,
            Expression::MemberAccess(m) => m.span,
            Expression::Call(c) => c.span,
            Expression::IndexAccess(i) => i.span,
            Expression::Range(r) => r.span,
            Expression::ForLoop(f) => f.span,
            Expression::SpreadForLoop(f) => f.span,
            Expression::Spread(s) => s.span,
            Expression::Match(m) => m.span,
            Expression::PrefixIncrement(i) => i.span,
            Expression::PostfixIncrement(i) => i.span,
        }
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct IndexAccess {
    pub object: Box<Expression>,
    pub index: Box<Expression>,
    pub span: Span,
}

impl IndexAccess {
    pub fn new(object: Expression, index: Expression, span: Span) -> Self {
        Self {
            object: Box::new(object),
            index: Box::new(index),
            span,
        }
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct MemberAccess {
    pub object: Box<Expression>,
    pub property: Identifier,
    pub span: Span,
}

impl MemberAccess {
    pub fn new(object: Expression, property: Identifier, span: Span) -> Self {
        Self {
            object: Box::new(object),
            property,
            span,
        }
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct Grouping {
    pub value: Box<Expression>,
    pub span: Span,
}

#[derive(PartialEq, Clone, Debug)]
pub struct Unary {
    pub operator: Operator,
    pub value: Box<Expression>,
    pub span: Span,
}

impl Unary {
    pub fn new(operator: Operator, value: Expression, span: Span) -> Unary {
        Unary {
            operator,
            value: Box::new(value),
            span,
        }
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct Binary {
    pub left: Box<Expression>,
    pub right: Box<Expression>,
    pub operator: Operator,
    pub span: Span,
}

impl Binary {
    pub fn new(left: Expression, operator: Operator, right: Expression, span: Span) -> Binary {
        Binary {
            left: Box::new(left),
            right: Box::new(right),
            operator,
            span,
        }
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct RangeExpression {
    pub start: Box<Expression>,
    pub end: Box<Expression>,
    pub span: Span,
}

impl RangeExpression {
    pub fn new(start: Expression, end: Expression, span: Span) -> RangeExpression {
        RangeExpression {
            start: Box::new(start),
            end: Box::new(end),
            span,
        }
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct ForLoopExpression {
    pub variable: Identifier,
    pub range_start: Box<Expression>,
    pub range_end: Box<Expression>,
    pub body: Block,
    pub span: Span,
}

impl ForLoopExpression {
    pub fn new(
        variable: Identifier,
        range_start: Expression,
        range_end: Expression,
        body: Block,
        span: Span,
    ) -> ForLoopExpression {
        ForLoopExpression {
            variable,
            range_start: Box::new(range_start),
            range_end: Box::new(range_end),
            body,
            span,
        }
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct MatchExpression {
    pub scrutinee: Box<Expression>,
    pub arms: Vec<(Expression, Block)>,
    pub span: Span,
}

impl MatchExpression {
    pub fn new(
        scrutinee: Expression,
        arms: Vec<(Expression, Block)>,
        span: Span,
    ) -> MatchExpression {
        MatchExpression {
            scrutinee: Box::new(scrutinee),
            arms,
            span,
        }
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct SpreadExpression {
    pub inner: Box<Expression>,
    pub span: Span,
}

#[derive(PartialEq, Clone, Debug)]
pub struct IncrementExpression {
    pub identifier: Identifier,
    pub span: Span,
}
