//! Lexical analysis: converts Ogham source text into a stream of tokens.

mod token;
mod token_type;

pub use token::*;
pub use token_type::*;

/// Scans Ogham source code and produces a vector of [`Token`]s.
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
            '-' => self.create_token(TokenType::Minus),
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
            '?' => self.create_token(TokenType::Question),
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
            '|' => {
                if self.match_next('|') {
                    self.consume(); // consume the second '|'
                    self.create_token(TokenType::Or)
                } else {
                    self.create_token(TokenType::Error("Unexpected character '|'".to_owned()))
                }
            }
            '&' => {
                if self.match_next('&') {
                    self.consume(); // consume the second '&'
                    self.create_token(TokenType::And)
                } else {
                    self.create_token(TokenType::Error("Unexpected character '&'".to_owned()))
                }
            }
            // Other
            '"' => self.consume_string(),
            _ => {
                if c.is_numeric() {
                    return self.consume_number();
                }
                if c.is_alphabetic() || c == '_' {
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

    /// Scans a string contained by quotation marks. Errors if the string is
    /// unterminated.
    ///
    /// Escape sequences (2026-07-08): `\"`, `\\`, `\n`, `\t`, and
    /// Rust-style `\u{HEX}`. Strings previously carried every byte
    /// verbatim, so authors writing Rust escape syntax (every UI corpus
    /// did) shipped the literal characters `\u{2026}` to the screen. A
    /// backslash before anything else stays literal — old corpuses with
    /// stray backslashes keep scanning unchanged.
    fn consume_string(&mut self) -> Token {
        // Skip the opening quotation mark.
        self.start += 1;

        let mut value = String::new();
        while !self.is_at_end() {
            let Some(next) = self.peek() else { break };
            if next == '"' {
                break;
            }
            if next != '\\' {
                value.push(next);
                self.consume();
                continue;
            }
            self.consume(); // the backslash
            match self.peek() {
                Some('"') => {
                    value.push('"');
                    self.consume();
                }
                Some('\\') => {
                    value.push('\\');
                    self.consume();
                }
                Some('n') => {
                    value.push('\n');
                    self.consume();
                }
                Some('t') => {
                    value.push('\t');
                    self.consume();
                }
                // `\u` only escapes in the full Rust shape `\u{HEX}`;
                // a bare `\u` stays literal like any unknown escape.
                Some('u') if self.input.get(self.current + 1) == Some(&'{') => {
                    self.consume(); // 'u'
                    self.consume(); // '{'
                    let mut hex = String::new();
                    while let Some(c) = self.peek() {
                        if c == '}' {
                            break;
                        }
                        hex.push(c);
                        self.consume();
                    }
                    if self.peek() != Some('}') {
                        return self.create_token(TokenType::Error(
                            "Unterminated \\u{…} escape in string".to_owned(),
                        ));
                    }
                    self.consume(); // '}'
                    match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        Some(c) => value.push(c),
                        None => {
                            return self.create_token(TokenType::Error(format!(
                                "Invalid unicode escape \\u{{{hex}}} in string"
                            )))
                        }
                    }
                }
                // Unknown escape (or trailing backslash): literal.
                _ => value.push('\\'),
            }
        }

        if self.is_at_end() {
            return self.create_token(TokenType::Error("Unterminated string".to_owned()));
        }

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
            // Typed-bindings keywords. `array` and `map` are intentionally
            // *not* keywords — they remain `Identifier` and the parser
            // recognizes them contextually only in type positions
            // (`array<T>`, `map<K, V>`). This avoids breaking any user
            // code that uses `array` or `map` as variable names.
            "record" => self.create_token(TokenType::Record),
            "host_state" => self.create_token(TokenType::HostState),
            "events" => self.create_token(TokenType::Events),
            // `screen` is a keyword; `view` is deliberately not. A screen's
            // body reads `view { ... }`, recognized contextually inside
            // `parse_screen_decl` — `view` is too plausible a variable name
            // to take from every document in every repo. `state` needs no
            // decision: it is already a keyword.
            "screen" => self.create_token(TokenType::Screen),
            "Self" => self.create_token(TokenType::SelfTy),
            _ => self.create_token(TokenType::Identifier(value)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(source: &str) -> Vec<Token> {
        let mut scanner = Scanner::new(source.to_string());
        scanner.scan()
    }

    #[test]
    fn scan_integer_literal() {
        let tokens = scan("42");
        assert_eq!(tokens[0].token_type, TokenType::Integer(42));
    }

    #[test]
    fn scan_float_literal() {
        let tokens = scan("3.14");
        assert_eq!(tokens[0].token_type, TokenType::Float(3.14));
    }

    #[test]
    fn scan_string_literal() {
        let tokens = scan(r#""hello""#);
        assert_eq!(tokens[0].token_type, TokenType::String("hello".to_string()));
    }

    #[test]
    fn scan_string_escapes() {
        let tokens = scan(r#""a\"b \\ c\nd\te \u{2212} \u{2026}""#);
        assert_eq!(
            tokens[0].token_type,
            TokenType::String("a\"b \\ c\nd\te − …".to_string())
        );
    }

    #[test]
    fn scan_string_keeps_unknown_escapes_literal() {
        // Old corpuses with stray backslashes must scan unchanged; a bare
        // \u without the {…} shape is just characters.
        let tokens = scan(r#""C:\path \q …""#);
        assert_eq!(
            tokens[0].token_type,
            TokenType::String(r"C:\path \q …".to_string())
        );
    }

    #[test]
    fn scan_string_rejects_bad_unicode_escapes() {
        for src in [r#""\u{2026""#, r#""\u{zz}""#, r#""\u{110000}""#] {
            let tokens = scan(src);
            assert!(
                matches!(tokens[0].token_type, TokenType::Error(_)),
                "{src} should scan to an error token, got {:?}",
                tokens[0].token_type
            );
        }
    }

    #[test]
    fn scan_boolean_literals() {
        let tokens = scan("true false");
        assert_eq!(tokens[0].token_type, TokenType::Boolean(true));
        assert_eq!(tokens[1].token_type, TokenType::Boolean(false));
    }

    #[test]
    fn scan_keywords() {
        let tokens = scan("let state if else return fn for in match import from log");
        assert_eq!(tokens[0].token_type, TokenType::Let);
        assert_eq!(tokens[1].token_type, TokenType::State);
        assert_eq!(tokens[2].token_type, TokenType::If);
        assert_eq!(tokens[3].token_type, TokenType::Else);
        assert_eq!(tokens[4].token_type, TokenType::Return);
        assert_eq!(tokens[5].token_type, TokenType::Fn);
        assert_eq!(tokens[6].token_type, TokenType::For);
        assert_eq!(tokens[7].token_type, TokenType::In);
        assert_eq!(tokens[8].token_type, TokenType::Match);
        assert_eq!(tokens[9].token_type, TokenType::Import);
        assert_eq!(tokens[10].token_type, TokenType::From);
        assert_eq!(tokens[11].token_type, TokenType::Log);
    }

    #[test]
    fn scan_arithmetic_operators() {
        let tokens = scan("+ - * /");
        assert_eq!(tokens[0].token_type, TokenType::Plus);
        assert_eq!(tokens[1].token_type, TokenType::Minus);
        assert_eq!(tokens[2].token_type, TokenType::Multiply);
        assert_eq!(tokens[3].token_type, TokenType::Divide);
    }

    #[test]
    fn scan_comparison_operators() {
        let tokens = scan("== != > >= < <=");
        assert_eq!(tokens[0].token_type, TokenType::EqualEqual);
        assert_eq!(tokens[1].token_type, TokenType::NotEqual);
        assert_eq!(tokens[2].token_type, TokenType::GreaterThan);
        assert_eq!(tokens[3].token_type, TokenType::GreaterThanOrEqualTo);
        assert_eq!(tokens[4].token_type, TokenType::LessThan);
        assert_eq!(tokens[5].token_type, TokenType::LessThanOrEqualTo);
    }

    #[test]
    fn scan_let_declaration() {
        let tokens = scan("let x = 5;");
        assert_eq!(tokens[0].token_type, TokenType::Let);
        assert_eq!(tokens[1].token_type, TokenType::Identifier("x".to_string()));
        assert_eq!(tokens[2].token_type, TokenType::Equal);
        assert_eq!(tokens[3].token_type, TokenType::Integer(5));
        assert_eq!(tokens[4].token_type, TokenType::Semicolon);
    }

    #[test]
    fn scan_identifier_with_underscores() {
        let tokens = scan("my_variable _private");
        assert_eq!(
            tokens[0].token_type,
            TokenType::Identifier("my_variable".to_string())
        );
        assert_eq!(
            tokens[1].token_type,
            TokenType::Identifier("_private".to_string())
        );
    }

    #[test]
    fn scan_range_and_spread() {
        let tokens = scan("0..10 ...");
        assert_eq!(tokens[0].token_type, TokenType::Integer(0));
        assert_eq!(tokens[1].token_type, TokenType::Range);
        assert_eq!(tokens[2].token_type, TokenType::Integer(10));
        assert_eq!(tokens[3].token_type, TokenType::Spread);
    }

    #[test]
    fn scan_fat_arrow() {
        let tokens = scan("=>");
        assert_eq!(tokens[0].token_type, TokenType::FatArrow);
    }

    #[test]
    fn scan_logical_operators() {
        let tokens = scan("&& ||");
        assert_eq!(tokens[0].token_type, TokenType::And);
        assert_eq!(tokens[1].token_type, TokenType::Or);
    }

    #[test]
    fn scan_lone_pipe_is_error() {
        let tokens = scan("|");
        assert!(matches!(tokens[0].token_type, TokenType::Error(_)));
    }

    #[test]
    fn scan_lone_ampersand_is_error() {
        let tokens = scan("&");
        assert!(matches!(tokens[0].token_type, TokenType::Error(_)));
    }

    // --- Typed-bindings keywords (Phase 1, M1a) ---

    #[test]
    fn scan_record_keyword() {
        let tokens = scan("record");
        assert_eq!(tokens[0].token_type, TokenType::Record);
    }

    #[test]
    fn scan_host_state_keyword_as_single_token() {
        let tokens = scan("host_state");
        assert_eq!(tokens[0].token_type, TokenType::HostState);
        // Must be a single token, not "host" + "_" + "state".
        assert_eq!(tokens[1].token_type, TokenType::EOF);
    }

    #[test]
    fn scan_events_keyword() {
        let tokens = scan("events");
        assert_eq!(tokens[0].token_type, TokenType::Events);
    }

    #[test]
    fn scan_self_ty_keyword() {
        let tokens = scan("Self");
        assert_eq!(tokens[0].token_type, TokenType::SelfTy);
    }

    #[test]
    fn array_and_map_remain_identifiers() {
        // Critical: keeping `array` and `map` as identifiers preserves
        // backward compatibility with any existing variable names.
        let tokens = scan("array map");
        assert_eq!(
            tokens[0].token_type,
            TokenType::Identifier("array".to_string())
        );
        assert_eq!(
            tokens[1].token_type,
            TokenType::Identifier("map".to_string())
        );
    }

    #[test]
    fn self_lowercase_remains_identifier() {
        // Only `Self` (capital S) is the keyword; `self` stays an identifier.
        let tokens = scan("self");
        assert_eq!(
            tokens[0].token_type,
            TokenType::Identifier("self".to_string())
        );
    }

    #[test]
    fn scan_typed_bindings_keywords_in_sequence() {
        let tokens = scan("record host_state events Self");
        assert_eq!(tokens[0].token_type, TokenType::Record);
        assert_eq!(tokens[1].token_type, TokenType::HostState);
        assert_eq!(tokens[2].token_type, TokenType::Events);
        assert_eq!(tokens[3].token_type, TokenType::SelfTy);
    }
}
