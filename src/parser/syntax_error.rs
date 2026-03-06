#[derive(Clone, Debug, PartialEq)]
pub struct SyntaxError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}
