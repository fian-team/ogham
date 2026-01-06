#[derive(Clone, Debug, PartialEq)]
pub enum TokenType {
    EOF,
    // Punctuation
    LeftParenthesis,
    RightParenthesis,
    LeftBracket,
    RightBracket,
    LeftSquareBracket,
    RightSquareBracket,
    Equal,
    EqualEqual, // ==
    Colon,
    Semicolon,
    Comma,
    // Arithmetic operators
    Plus,      // +
    Minus,     // -
    Multiply,  // *
    Divide,    // /
    Modulo,    // %
    Power,     // ^
    Arrow,     // ->
    Increment, // ++
    // Comparison operators
    GreaterThan,          // >
    GreaterThanOrEqualTo, // >=
    LessThan,             // <
    LessThanOrEqualTo,    // <=
    Not,                  // !
    NotEqual,             // !=
    // Keywords
    State,
    Let,
    If,
    Else,
    Return,
    Log,
    Fn,
    // String delimiters
    Quote,
    // Literals
    Identifier(String),
    String(String),
    Integer(i32),
    Float(f64),
    Boolean(bool),
    // Other
    Error,
}
