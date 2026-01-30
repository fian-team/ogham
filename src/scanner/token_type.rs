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
    Dot,
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
    Range,     // ..
    Spread,    // ...
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
    For,
    In,
    Match,
    Import,
    From,
    // Match arms
    FatArrow, // =>
    // String delimiters
    Quote,
    // Literals
    Identifier(String),
    String(String),
    Integer(i32),
    Float(f64),
    Boolean(bool),
    // Other
    /// Scanner error token.
    ///
    /// This is emitted when the scanner encounters an unexpected character or
    /// an unterminated construct (e.g. string/comment).
    Error(String),
}
