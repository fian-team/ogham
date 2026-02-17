use super::token_type::TokenType;

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub token_type: TokenType,
    pub line: usize,
    pub column: usize,
    pub start: usize,
    pub length: usize,
}
