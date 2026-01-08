#[derive(PartialEq, Clone, Debug)]
pub enum Operator {
    // Equality
    Equals,
    NotEquals,
    // Comparison
    GreaterThan,
    GreaterThanOrEqualTo,
    LessThan,
    LessThanOrEqualTo,
    // Term
    Plus,
    Minus,
    // Factor
    Multiply,
    Divide,
    // Unary
    Not,
}
