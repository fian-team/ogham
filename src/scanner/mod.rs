mod token;
mod token_type;

pub use token::*;
pub use token_type::*;

/// Takes an Iris script as input and parses it into a vector of tokens.
pub struct Scanner {
    /// The input string; the Iris script to be scanned, in other words.
    input: Vec<char>,
    /// The character at the beginning of the token currently being scanned. For instance,
    /// if we were scanning the sequence `let varName: int = 5;`, we might seen `start` be
    /// equal to `4` while scanning `varName`.
    start: usize,
    /// The next character to be scanned.
    current: usize,
    /// The line the scanner is currently scanning in the input string.
    line: usize,
    /// The position of the last newline character, used to calculate column positions.
    last_newline: usize,
}

impl Scanner {
    /// Requires an script in string form as input.
    pub fn new(input: String) -> Scanner {
        Scanner {
            input: input.chars().collect(),
            start: 0,
            current: 0,
            line: 0,
            last_newline: 0,
        }
    }

    /// Scans the input string provided to the constructor and returns a vec of tokens.
    pub fn scan(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        loop {
            let token = self.scan_token();
            tokens.push(token.clone());
            if token.token_type == TokenType::EOF {
                break;
            }
        }
        return tokens;
    }

    /// Scans the next token, skipping any whitespace.
    pub fn scan_token(&mut self) -> Token {
        self.skip_whitespace();

        if self.is_at_end() {
            // Important: `skip_whitespace()` may have consumed trailing newlines and advanced
            // `last_newline`. If we create EOF with an older `start` (from the previous token),
            // `start - last_newline` can underflow when computing the column.
            self.start = self.current;
            return self.create_token(TokenType::EOF);
        }

        self.start = self.current;

        let c = self.consume();

        match c {
            // Singles
            '(' => self.create_token(TokenType::LeftParenthesis),
            ')' => self.create_token(TokenType::RightParenthesis),
            '{' => self.create_token(TokenType::LeftBracket),
            '}' => self.create_token(TokenType::RightBracket),
            '[' => self.create_token(TokenType::LeftSquareBracket),
            ']' => self.create_token(TokenType::RightSquareBracket),
            '+' => {
                if self.match_next('+') {
                    self.consume(); // consume the second '+'
                    self.create_token(TokenType::Increment)
                } else {
                    self.create_token(TokenType::Plus)
                }
            }
            '-' => {
                if self.match_next('>') {
                    self.consume(); // consume the '>'
                    self.create_token(TokenType::Arrow)
                } else {
                    self.create_token(TokenType::Minus)
                }
            }
            '*' => self.create_token(TokenType::Multiply),
            '/' => {
                if self.match_next('/') {
                    // Single-line comment
                    self.consume(); // consume the second '/'
                    self.consume_comment_line();
                    return self.scan_token(); // Recursively scan next token after comment
                } else if self.match_next('*') {
                    // Multi-line comment
                    self.consume(); // consume the '*'
                    if self.consume_comment_block() {
                        return self.scan_token(); // Recursively scan next token after comment
                    } else {
                        return self.create_token(TokenType::Error(
                            "Unterminated block comment".to_owned(),
                        ));
                    }
                } else {
                    self.create_token(TokenType::Divide)
                }
            }
            '%' => self.create_token(TokenType::Modulo),
            '^' => self.create_token(TokenType::Power),
            '.' => {
                // Check if next character is also '.' using peek (like other multi-char tokens)
                if let Some(next_char) = self.peek() {
                    if next_char == '.' {
                        self.consume(); // consume the second '.'
                                        // Check if next character is also '.' (for ... spread operator)
                        if let Some(third_char) = self.peek() {
                            if third_char == '.' {
                                self.consume(); // consume the third '.'
                                self.create_token(TokenType::Spread)
                            } else {
                                self.create_token(TokenType::Range)
                            }
                        } else {
                            self.create_token(TokenType::Range)
                        }
                    } else {
                        self.create_token(TokenType::Dot)
                    }
                } else {
                    self.create_token(TokenType::Dot)
                }
            }
            ':' => self.create_token(TokenType::Colon),
            ';' => self.create_token(TokenType::Semicolon),
            ',' => self.create_token(TokenType::Comma),
            '=' => {
                if self.match_next('>') {
                    self.consume(); // consume the '>'
                    self.create_token(TokenType::FatArrow)
                } else if self.match_next('=') {
                    self.consume(); // consume the second '='
                    self.create_token(TokenType::EqualEqual)
                } else {
                    self.create_token(TokenType::Equal)
                }
            }
            '>' => {
                if self.match_next('=') {
                    self.consume(); // consume the '='
                    self.create_token(TokenType::GreaterThanOrEqualTo)
                } else {
                    self.create_token(TokenType::GreaterThan)
                }
            }
            '<' => {
                if self.match_next('=') {
                    self.consume(); // consume the '='
                    self.create_token(TokenType::LessThanOrEqualTo)
                } else {
                    self.create_token(TokenType::LessThan)
                }
            }
            '!' => {
                if self.match_next('=') {
                    self.consume(); // consume the '='
                    self.create_token(TokenType::NotEqual)
                } else {
                    self.create_token(TokenType::Not)
                }
            }
            // Other
            '"' => self.consume_string(),
            _ => {
                if c.is_numeric() {
                    return self.consume_number();
                }
                if c.is_alphabetic() {
                    return self.consume_keyword_or_identifier();
                }
                self.create_token(TokenType::Error(format!("Unexpected character {:?}", c)))
            }
        }
    }

    /// Returns true if the scanner is at the end of its input.
    fn is_at_end(&self) -> bool {
        return self.current >= self.input.len();
    }

    /// Skips any number of consecutive whitespace tokens ahead of the scanner's current position.
    fn skip_whitespace(&mut self) {
        while !self.is_at_end() {
            if let Some(c) = self.peek() {
                if !c.is_whitespace() {
                    break;
                }
                if c == '\n' {
                    self.line += 1;
                    self.last_newline = self.current + 1;
                }
                self.current += 1;
            } else {
                break;
            }
        }
    }

    /// Returns Some<char> if the scanner is not at the end of its input. Returns None if the
    /// scanner is at the end of its input.
    fn peek(&self) -> Option<char> {
        if self.is_at_end() {
            return None;
        }
        Some(self.input[self.current])
    }

    /// Advances the scanner through the next character and returns the consumed character.
    fn consume(&mut self) -> char {
        let c = self.input[self.current];
        if c == '\n' {
            self.line += 1;
            self.last_newline = self.current + 1;
        }
        self.current += 1;
        c
    }

    /// Returns true if the next token matches the expected token. Returns false otherwise or
    /// if the scanner is at the end of its input.
    fn match_next(&mut self, expected: char) -> bool {
        if self.is_at_end() {
            return false;
        }
        if self.input[self.current] != expected {
            return false;
        }
        true
    }

    /// Consumes a single-line comment (// ...) until the end of the line.
    fn consume_comment_line(&mut self) {
        while !self.is_at_end() {
            let next = self.peek();
            if let Some(c) = next {
                if c == '\n' {
                    self.consume();
                    break;
                }
                self.consume();
            } else {
                break;
            }
        }
    }

    /// Consumes a multi-line comment (/* ... */). Returns true if the comment was properly
    /// terminated, false if it was unterminated. Assumes the '*' has already been consumed.
    fn consume_comment_block(&mut self) -> bool {
        while !self.is_at_end() {
            let next = self.peek();
            if let Some(c) = next {
                if c == '*' {
                    self.consume(); // consume the '*'
                    if !self.is_at_end() && self.peek() == Some('/') {
                        self.consume(); // consume the '/'
                        return true;
                    }
                } else {
                    self.consume();
                }
            } else {
                break;
            }
        }
        false
    }

    /// Creates a token with the provided token type.
    /// Line and column are 1-indexed for user-facing error messages.
    fn create_token(&self, token_type: TokenType) -> Token {
        // Calculate column as the number of characters from the last newline to the start of the token
        let column = self.start - self.last_newline;
        Token {
            token_type,
            line: self.line + 1,
            start: self.start,
            length: self.current - self.start,
            column: column + 1, // Convert to 1-indexed
        }
    }

    /// Scans a string contained by quotation marks. Errors if the string is unterminated.
    fn consume_string(&mut self) -> Token {
        // Skip the opening quotation mark.
        self.start += 1;

        while !self.is_at_end() {
            let peek = self.peek();
            if let Some(next) = peek {
                if next == '"' {
                    break;
                }
                self.consume();
            }
        }

        if self.is_at_end() {
            return self.create_token(TokenType::Error("Unterminated string".to_owned()));
        }

        let value: String = self.input[self.start..self.current]
            .into_iter()
            .collect::<String>()
            .clone();

        self.consume();
        self.create_token(TokenType::String(value))
    }

    /// Consumes a number and returns an integer or float token.
    fn consume_number(&mut self) -> Token {
        while !self.is_at_end() && self.peek().unwrap().is_numeric() {
            self.consume();
        }

        // Check for decimal point
        if !self.is_at_end() && self.peek() == Some('.') {
            // Check if the next character is also '.' (which would be a range token '..')
            // If so, don't consume the '.' and return an integer token instead
            if self.current + 1 < self.input.len() && self.input[self.current + 1] == '.' {
                // This is a range token, not a decimal point
                let value_as_string = self.input[self.start..self.current]
                    .into_iter()
                    .collect::<String>()
                    .clone();
                let value: i32 = value_as_string.parse().unwrap();
                return self.create_token(TokenType::Integer(value));
            }
            self.consume(); // consume the '.'
                            // Consume digits after decimal point
            while !self.is_at_end() && self.peek().unwrap().is_numeric() {
                self.consume();
            }
            let value_as_string = self.input[self.start..self.current]
                .into_iter()
                .collect::<String>()
                .clone();
            let value: f64 = value_as_string.parse().unwrap();
            return self.create_token(TokenType::Float(value));
        }

        let value_as_string = self.input[self.start..self.current]
            .into_iter()
            .collect::<String>()
            .clone();

        let value: i32 = value_as_string.parse().unwrap();
        self.create_token(TokenType::Integer(value))
    }

    /// Scans a sequence of characters. If it's a keyword, returns the appropriate
    /// keyword token. Otherwise, returns an identifier token.
    fn consume_keyword_or_identifier(&mut self) -> Token {
        while !self.is_at_end() {
            let next = self.peek();
            if let Some(c) = next {
                if c.is_alphanumeric() || c == '_' {
                    self.consume();
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        let value = self.input[self.start..self.current]
            .into_iter()
            .collect::<String>()
            .clone();

        // Use string comparison for keywords (more reliable than slice pattern matching)
        match value.as_str() {
            "let" => self.create_token(TokenType::Let),
            "state" => self.create_token(TokenType::State),
            "if" => self.create_token(TokenType::If),
            "else" => self.create_token(TokenType::Else),
            "return" => self.create_token(TokenType::Return),
            "log" => self.create_token(TokenType::Log),
            "fn" => self.create_token(TokenType::Fn),
            "for" => self.create_token(TokenType::For),
            "in" => self.create_token(TokenType::In),
            "match" => self.create_token(TokenType::Match),
            "import" => self.create_token(TokenType::Import),
            "from" => self.create_token(TokenType::From),
            "true" => self.create_token(TokenType::Boolean(true)),
            "false" => self.create_token(TokenType::Boolean(false)),
            _ => self.create_token(TokenType::Identifier(value)),
        }
    }
}
