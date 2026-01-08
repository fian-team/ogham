#[derive(Clone, Debug, PartialEq)]
pub struct SyntaxError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl SyntaxError {
    pub fn get_message(&self) -> String {
        self.message.clone()
    }
}
