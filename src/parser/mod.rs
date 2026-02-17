//! Parser: transforms a token stream into an abstract syntax tree (AST).
//!
//! The parser consumes [`scanner::Token`]s and produces a tree of
//! [`Statement`] / [`Expression`] nodes that the compiler or tree-walker
//! can evaluate.

mod array;
mod block;
mod call;
mod expression;
mod function;
mod identifier;
mod literal;
mod map;
mod node;
mod operator;
mod statement;
mod syntax_error;
mod widget;

pub use {
    array::*, block::*, call::*, expression::*, function::*, identifier::*, literal::*, map::*,
    operator::*, statement::*, syntax_error::*, widget::*,
};

use super::scanner;

/// Recursive-descent parser for the Ogham language.
#[derive(PartialEq, Clone, Debug)]
pub struct Parser {
    input: Vec<scanner::Token>,
    current: usize,
    module: Function,
    /// This is used to handle the case where an identifier is followed by a `{` token, which
    /// can be parsed as either part of a match expression or a widget expression.
    /// When parsing a match expression, we do not want to accidentally parse the scrutinee as a widget.
    parsing_match_scrutinee: bool,
}

impl Parser {
    pub fn new(input: Vec<scanner::Token>) -> Parser {
        Parser {
            input,
            current: 0,
            module: Function::new(),
            parsing_match_scrutinee: false,
        }
    }

    fn current(&self) -> Option<scanner::Token> {
        if self.input.len() > self.current {
            Some(self.input[self.current].clone())
        } else {
            None
        }
    }

    fn next_is(&self, types: Vec<scanner::TokenType>) -> bool {
        if self.current >= self.input.len() {
            return false;
        }
        if types.contains(&self.current().unwrap().token_type) {
            true
        } else {
            false
        }
    }

    fn get_current_as_operator(&mut self) -> Result<Operator, SyntaxError> {
        let token = self.current().unwrap();
        let line = token.line;
        let column = token.column;
        let token_type = token.token_type;
        self.current += 1;
        match token_type {
            scanner::TokenType::EqualEqual => Ok(Operator::Equals),
            scanner::TokenType::GreaterThan => Ok(Operator::GreaterThan),
            scanner::TokenType::GreaterThanOrEqualTo => Ok(Operator::GreaterThanOrEqualTo),
            scanner::TokenType::LessThan => Ok(Operator::LessThan),
            scanner::TokenType::LessThanOrEqualTo => Ok(Operator::LessThanOrEqualTo),
            scanner::TokenType::Plus => Ok(Operator::Plus),
            scanner::TokenType::Minus => Ok(Operator::Minus),
            scanner::TokenType::Multiply => Ok(Operator::Multiply),
            scanner::TokenType::Divide => Ok(Operator::Divide),
            scanner::TokenType::Not => Ok(Operator::Not),
            scanner::TokenType::NotEqual => Ok(Operator::NotEquals),
            _ => Err(SyntaxError {
                message: "Invalid operator".to_owned(),
                line,
                column,
            }),
        }
    }

    pub fn parse(&mut self) -> Result<Function, SyntaxError> {
        let block = self.parse_block(true)?;
        self.module.body = block;
        Ok(self.module.clone())
    }

    pub fn parse_block(&mut self, allow_import: bool) -> Result<Block, SyntaxError> {
        let mut block = Block::new();
        while self.current < self.input.len() {
            if let scanner::TokenType::EOF = self.input[self.current].token_type.clone() {
                break;
            }
            // Check if next token is a closing bracket (end of block)
            if let scanner::TokenType::RightBracket = self.input[self.current].token_type.clone() {
                break;
            }
            let statement = self.parse_statement(allow_import)?;
            block.statement_list.push(statement);
        }
        Ok(block)
    }

    pub fn parse_statement(&mut self, allow_import: bool) -> Result<Statement, SyntaxError> {
        let current_token = &self.input[self.current];
        match current_token.token_type.clone() {
            scanner::TokenType::Import => {
                if !allow_import {
                    let t = current_token.clone();
                    return Err(SyntaxError {
                        message: "import is only allowed at module top level".to_owned(),
                        line: t.line,
                        column: t.column,
                    });
                }
                self.parse_import()
            }
            scanner::TokenType::If => self.parse_conditional(),
            scanner::TokenType::Return => self.parse_return(),
            scanner::TokenType::Let => self.parse_let(),
            scanner::TokenType::State => self.parse_state(),
            // scanner::TokenType::Event => self.parse_event(),
            scanner::TokenType::Identifier(_) => self.parse_identifier_statement(),
            scanner::TokenType::Log => self.parse_log(),
            scanner::TokenType::For => self.parse_for_loop_statement(),
            _ => {
                // Try to parse as an expression statement (for implicit returns)
                self.parse_expression_statement()
            }
        }
    }

    fn parse_import(&mut self) -> Result<Statement, SyntaxError> {
        self.consume_if(scanner::TokenType::Import)?;
        let (names, path) = if self.next_is(vec![scanner::TokenType::LeftBracket]) {
            self.consume_if(scanner::TokenType::LeftBracket)?;
            let mut ids = Vec::new();
            loop {
                if self.next_is(vec![scanner::TokenType::RightBracket]) {
                    self.consume_if(scanner::TokenType::RightBracket)?;
                    break;
                }
                let id = self.consume_if_identifier()?.get();
                ids.push(id);
                if self.next_is(vec![scanner::TokenType::RightBracket]) {
                    self.consume_if(scanner::TokenType::RightBracket)?;
                    break;
                }
                self.consume_if(scanner::TokenType::Comma)?;
            }
            self.consume_if(scanner::TokenType::From)?;
            let path_token = self.current().ok_or_else(|| SyntaxError {
                message: "Expected string path after 'from'".to_owned(),
                line: 0,
                column: 0,
            })?;
            let path = match &path_token.token_type {
                scanner::TokenType::String(s) => {
                    self.consume();
                    s.clone()
                }
                _ => {
                    return Err(SyntaxError {
                        message: "Expected string path after 'from'".to_owned(),
                        line: path_token.line,
                        column: path_token.column,
                    });
                }
            };
            (Some(ids), path)
        } else {
            let path_token = self.current().ok_or_else(|| SyntaxError {
                message: "Expected string path or '{' after 'import'".to_owned(),
                line: 0,
                column: 0,
            })?;
            let path = match &path_token.token_type {
                scanner::TokenType::String(s) => {
                    self.consume();
                    s.clone()
                }
                _ => {
                    return Err(SyntaxError {
                        message: "Expected string path or '{' after 'import'".to_owned(),
                        line: path_token.line,
                        column: path_token.column,
                    });
                }
            };
            (None, path)
        };
        self.consume_if(scanner::TokenType::Semicolon)?;
        Ok(Statement::new_import(names, path))
    }

    fn consume_if(&mut self, token: scanner::TokenType) -> Result<(), SyntaxError> {
        if self.next_is(vec![token.clone()]) {
            self.consume();
        } else {
            let current_token = self.current().unwrap();
            return Err(SyntaxError {
                message: format!(
                    "Expected token {:#?}, received {:#?}",
                    token.clone(),
                    current_token.token_type
                )
                .to_owned(),
                column: current_token.column,
                line: current_token.line,
            });
        }
        Ok(())
    }

    fn consume_if_identifier(&mut self) -> Result<Identifier, SyntaxError> {
        let current_token = self.current().unwrap();
        if let scanner::TokenType::Identifier(identifier) = current_token.token_type {
            self.consume();
            Ok(Identifier::new(&identifier))
        } else {
            Err(SyntaxError {
                message: "Expected identifier".to_owned(),
                line: current_token.line,
                column: current_token.column,
            })
        }
    }

    fn peek(&self) -> scanner::TokenType {
        if self.current == self.input.len() {
            return scanner::TokenType::EOF;
        }
        self.input[self.current + 1].token_type.clone()
    }

    fn consume(&mut self) {
        self.current += 1;
    }

    // Check if we're at the end of a block (EOF or RightBracket)
    fn is_at_block_end(&self) -> bool {
        self.current >= self.input.len()
            || matches!(
                self.input[self.current].token_type,
                scanner::TokenType::EOF | scanner::TokenType::RightBracket
            )
    }

    // Convert an expression to a statement, handling implicit returns.
    // If there's a semicolon, consume it and return as expression statement.
    // If we're at the end of block without semicolon, treat as implicit return.
    // Otherwise, return as expression statement.
    fn expression_to_statement(
        &mut self,
        expression: Expression,
    ) -> Result<Statement, SyntaxError> {
        if self.next_is(vec![scanner::TokenType::Semicolon]) {
            self.consume_if(scanner::TokenType::Semicolon)?;
            Ok(Statement::new_expression(expression))
        } else if self.is_at_block_end() {
            // Implicit return
            Ok(Statement::new_return(Some(expression)))
        } else {
            Ok(Statement::new_expression(expression))
        }
    }

    fn parse_optional_type(&mut self) -> Result<Identifier, SyntaxError> {
        if self.next_is(vec![scanner::TokenType::Colon]) {
            self.consume_if(scanner::TokenType::Colon)?;
            self.parse_type_identifier()
        } else {
            Ok(Identifier::new("infer"))
        }
    }

    /// Parses a type identifier, including postfix array syntax like `int[]` or `widget[][]`.
    ///
    /// Internally, types are represented as an `Identifier` string, so `int[][]` is stored
    /// as the identifier `"int[][]"`.
    fn parse_type_identifier(&mut self) -> Result<Identifier, SyntaxError> {
        let base = self.consume_if_identifier()?;
        let mut type_str = base.get();

        while self.next_is(vec![scanner::TokenType::LeftSquareBracket]) {
            self.consume_if(scanner::TokenType::LeftSquareBracket)?;
            // For types, we only support empty array brackets: `[]`
            self.consume_if(scanner::TokenType::RightSquareBracket)?;
            type_str.push_str("[]");
        }

        Ok(Identifier::new(&type_str))
    }

    // When an identifier is used as a statement, it can be an assignment,
    // a function call, or a widget expression.
    fn parse_identifier_statement(&mut self) -> Result<Statement, SyntaxError> {
        let identifier = self.consume_if_identifier()?;

        // Check what the next token is (after consuming the identifier)
        let next_token_type = if self.current < self.input.len() {
            Some(&self.input[self.current].token_type)
        } else {
            None
        };

        match next_token_type {
            Some(scanner::TokenType::LeftParenthesis) => {
                let expr = Expression::Literal(Literal::Identifier(identifier));
                let full_expr = self.parse_postfix(expr)?;
                self.expression_to_statement(full_expr)
            }
            Some(scanner::TokenType::LeftBracket) => {
                // Widget expression
                let widget = self.parse_widget(identifier)?;
                self.expression_to_statement(Expression::Widget(widget))
            }
            Some(scanner::TokenType::Equal) => {
                // Assignment
                self.parse_assign(identifier)
            }
            _ => {
                // Not a call, widget, or assignment - treat as identifier expression
                // This handles cases like standalone identifiers
                let expr = Expression::Literal(Literal::Identifier(identifier));
                self.expression_to_statement(expr)
            }
        }
    }

    // Parse an expression statement with optional semicolon.
    // If the expression is followed by EOF or RightBracket (end of block) without a semicolon,
    // it's treated as an implicit return.
    fn parse_expression_statement(&mut self) -> Result<Statement, SyntaxError> {
        let expression = self.expression()?;
        self.expression_to_statement(expression)
    }

    pub fn parse_conditional(&mut self) -> Result<Statement, SyntaxError> {
        // Parse: if expression { block }
        self.consume_if(scanner::TokenType::If)?;
        let condition = self.expression()?;
        self.consume_if(scanner::TokenType::LeftBracket)?;
        let block = self.parse_block(false)?;
        self.consume_if(scanner::TokenType::RightBracket)?;

        let mut branches = vec![(condition, block)];
        let mut else_block = None;

        // Parse optional else if branches
        while self.next_is(vec![scanner::TokenType::Else]) {
            // Check if next token after Else is If
            if self.current + 1 < self.input.len() {
                if let scanner::TokenType::If = self.input[self.current + 1].token_type {
                    // Parse: else if expression { block }
                    self.consume_if(scanner::TokenType::Else)?;
                    self.consume_if(scanner::TokenType::If)?;
                    let else_if_condition = self.expression()?;
                    self.consume_if(scanner::TokenType::LeftBracket)?;
                    let else_if_block = self.parse_block(false)?;
                    self.consume_if(scanner::TokenType::RightBracket)?;
                    branches.push((else_if_condition, else_if_block));
                } else {
                    // Parse: else { block }
                    self.consume_if(scanner::TokenType::Else)?;
                    self.consume_if(scanner::TokenType::LeftBracket)?;
                    else_block = Some(self.parse_block(false)?);
                    self.consume_if(scanner::TokenType::RightBracket)?;
                    break;
                }
            } else {
                // Parse: else { block }
                self.consume_if(scanner::TokenType::Else)?;
                self.consume_if(scanner::TokenType::LeftBracket)?;
                else_block = Some(self.parse_block(false)?);
                self.consume_if(scanner::TokenType::RightBracket)?;
                break;
            }
        }

        Ok(Statement::new_conditional(branches, else_block))
    }

    pub fn parse_return(&mut self) -> Result<Statement, SyntaxError> {
        self.consume_if(scanner::TokenType::Return)?;
        if self.next_is(vec![scanner::TokenType::Semicolon]) {
            self.consume_if(scanner::TokenType::Semicolon)?;
            return Ok(Statement::new_return(None));
        }
        let expression = self.expression()?;
        self.consume_if(scanner::TokenType::Semicolon)?;
        return Ok(Statement::new_return(Some(expression)));
    }

    pub fn parse_let(&mut self) -> Result<Statement, SyntaxError> {
        self.consume_if(scanner::TokenType::Let)?;
        let (variable_name, _variable_type, variable_value) =
            self.parse_remainder_of_declaration()?;
        return Ok(Statement::Declare(DeclareStatement::new(
            &variable_name,
            variable_value,
        )));
    }

    pub fn parse_state(&mut self) -> Result<Statement, SyntaxError> {
        self.consume_if(scanner::TokenType::State)?;
        let (variable_name, _variable_type, variable_value) =
            self.parse_remainder_of_declaration()?;
        return Ok(Statement::DeclareState(DeclareStateStatement::new(
            &variable_name,
            variable_value,
        )));
    }

    fn parse_remainder_of_declaration(
        &mut self,
    ) -> Result<(Identifier, Identifier, Expression), SyntaxError> {
        let identifier = self.consume_if_identifier()?;
        let identifier_type = self.parse_optional_type()?;
        self.consume_if(scanner::TokenType::Equal)?;
        let expression = self.expression()?;
        self.consume_if(scanner::TokenType::Semicolon)?;
        return Ok((identifier, identifier_type, expression));
    }

    pub fn parse_assign(&mut self, identifier: Identifier) -> Result<Statement, SyntaxError> {
        self.consume_if(scanner::TokenType::Equal)?;
        let expression = self.expression()?;
        self.consume_if(scanner::TokenType::Semicolon)?;
        return Ok(Statement::new_assign(&identifier, expression));
    }

    // pub fn parse_event(&mut self) -> Result<Statement, SyntaxError> {
    //   self.consume_if(scanner::TokenType::Event)?;
    //   let event_identifier = self.consume_if_identifier()?;
    //   self.consume_if(scanner::TokenType::Equal)?;
    //   let function = self.parse_function()?;
    //   return Ok(Statement::new_event(&event_identifier, function));
    // }

    pub fn parse_log(&mut self) -> Result<Statement, SyntaxError> {
        self.consume_if(scanner::TokenType::Log)?;
        let expression = self.expression()?;
        self.consume_if(scanner::TokenType::Semicolon)?;
        return Ok(Statement::new_log(expression));
    }

    pub fn parse_for_loop_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.consume_if(scanner::TokenType::For)?;
        self.consume_if(scanner::TokenType::LeftParenthesis)?;
        let variable = self.consume_if_identifier()?;
        self.consume_if(scanner::TokenType::In)?;
        // Parse range explicitly: start..end (factor stops before .. so we don't consume it here)
        let range_start = self.factor()?;
        self.consume_if(scanner::TokenType::Range)?;
        let range_end = self.factor()?;
        self.consume_if(scanner::TokenType::RightParenthesis)?;
        self.consume_if(scanner::TokenType::LeftBracket)?;
        let body = self.parse_block(false)?;
        self.consume_if(scanner::TokenType::RightBracket)?;
        return Ok(Statement::new_for_loop(
            variable,
            range_start,
            range_end,
            body,
        ));
    }

    fn parse_for_loop_expression(&mut self, is_spread: bool) -> Result<Expression, SyntaxError> {
        if is_spread {
            self.consume_if(scanner::TokenType::Spread)?;
        }
        self.consume_if(scanner::TokenType::For)?;
        self.consume_if(scanner::TokenType::LeftParenthesis)?;
        let variable = self.consume_if_identifier()?;
        self.consume_if(scanner::TokenType::In)?;
        // Parse range explicitly: start..end (factor stops before .. so we don't consume it here)
        let range_start = self.factor()?;
        self.consume_if(scanner::TokenType::Range)?;
        let range_end = self.factor()?;
        self.consume_if(scanner::TokenType::RightParenthesis)?;
        self.consume_if(scanner::TokenType::LeftBracket)?;
        let body = self.parse_block(false)?;
        self.consume_if(scanner::TokenType::RightBracket)?;
        if is_spread {
            Ok(Expression::new_spread_for_loop(
                variable,
                range_start,
                range_end,
                body,
            ))
        } else {
            Ok(Expression::new_for_loop(
                variable,
                range_start,
                range_end,
                body,
            ))
        }
    }

    /// Parse a match pattern (primary-only: literal or identifier including _).
    fn parse_match_pattern(&mut self) -> Result<Expression, SyntaxError> {
        let current_token = self.current().ok_or_else(|| SyntaxError {
            message: "Unexpected end of input while parsing match pattern".to_owned(),
            line: 0,
            column: 0,
        })?;
        let expr = match &current_token.token_type {
            scanner::TokenType::Integer(value) => {
                self.current += 1;
                Expression::Literal(Literal::Integer(*value))
            }
            scanner::TokenType::Float(value) => {
                self.current += 1;
                Expression::Literal(Literal::Float(*value))
            }
            scanner::TokenType::Boolean(value) => {
                self.current += 1;
                Expression::Literal(Literal::Boolean(*value))
            }
            scanner::TokenType::String(value) => {
                self.current += 1;
                Expression::Literal(Literal::String(value.clone()))
            }
            scanner::TokenType::Identifier(value) => {
                self.current += 1;
                Expression::Literal(Literal::Identifier(Identifier::new(&value)))
            }
            _ => {
                return Err(SyntaxError {
                    message: format!(
                        "Expected match pattern (literal or identifier), got {:?}",
                        current_token.token_type
                    ),
                    line: current_token.line,
                    column: current_token.column,
                });
            }
        };
        Ok(expr)
    }

    fn parse_match_expression(&mut self) -> Result<Expression, SyntaxError> {
        // Only accept Match token to begin the expression
        let current = self.current().ok_or_else(|| SyntaxError {
            message: "Expected 'match'".to_owned(),
            line: 0,
            column: 0,
        })?;
        match &current.token_type {
            scanner::TokenType::Match => {
                self.consume();
            }
            _ => {
                return Err(SyntaxError {
                    message: "Expected 'match'".to_owned(),
                    line: current.line,
                    column: current.column,
                });
            }
        }
        self.parsing_match_scrutinee = true;
        let scrutinee = self.expression();
        self.parsing_match_scrutinee = false;
        let scrutinee = scrutinee?;
        self.consume_if(scanner::TokenType::LeftBracket)?;

        let mut arms = Vec::new();
        while !self.next_is(vec![scanner::TokenType::RightBracket]) {
            let pattern = self.parse_match_pattern()?;
            self.consume_if(scanner::TokenType::FatArrow)?;

            let body = if self.next_is(vec![scanner::TokenType::LeftBracket]) {
                self.consume_if(scanner::TokenType::LeftBracket)?;
                let block = self.parse_block(false)?;
                self.consume_if(scanner::TokenType::RightBracket)?;
                block
            } else {
                let expr = self.expression()?;
                let mut block = Block::new();
                block.statement_list.push(Statement::new_return(Some(expr)));
                block
            };

            arms.push((pattern, body));

            if self.next_is(vec![scanner::TokenType::Comma]) {
                self.consume_if(scanner::TokenType::Comma)?;
            }
        }

        self.consume_if(scanner::TokenType::RightBracket)?;
        Ok(Expression::new_match(scrutinee, arms))
    }

    pub fn expression(&mut self) -> Result<Expression, SyntaxError> {
        self.equality()
    }

    pub fn equality(&mut self) -> Result<Expression, SyntaxError> {
        let mut expression = self.comparison()?;
        while self.next_is(vec![
            scanner::TokenType::EqualEqual,
            scanner::TokenType::NotEqual,
        ]) {
            let operator = self.get_current_as_operator()?;
            let right = self.comparison()?;
            expression = Expression::new_binary(expression, operator, right);
        }
        Ok(expression)
    }

    pub fn comparison(&mut self) -> Result<Expression, SyntaxError> {
        let mut expression = self.term()?;
        while self.next_is(vec![
            scanner::TokenType::GreaterThan,
            scanner::TokenType::GreaterThanOrEqualTo,
            scanner::TokenType::LessThan,
            scanner::TokenType::LessThanOrEqualTo,
        ]) {
            let operator = self.get_current_as_operator()?;
            let right = self.term()?;
            expression = Expression::new_binary(expression, operator, right);
        }
        Ok(expression)
    }

    pub fn term(&mut self) -> Result<Expression, SyntaxError> {
        let mut expression = self.range()?;
        while self.next_is(vec![scanner::TokenType::Plus, scanner::TokenType::Minus]) {
            let operator = self.get_current_as_operator()?;
            let right = self.range()?;
            expression = Expression::new_binary(expression, operator, right);
        }
        Ok(expression)
    }

    pub fn factor(&mut self) -> Result<Expression, SyntaxError> {
        let mut expression = self.unary()?;
        while self.next_is(vec![
            scanner::TokenType::Multiply,
            scanner::TokenType::Divide,
        ]) {
            let operator = self.get_current_as_operator()?;
            let right = self.unary()?;
            expression = Expression::new_binary(expression, operator, right);
        }
        Ok(expression)
    }

    pub fn range(&mut self) -> Result<Expression, SyntaxError> {
        let start = self.factor()?;
        if self.next_is(vec![scanner::TokenType::Range]) {
            self.consume(); // consume Range token
            let end = self.factor()?;
            Ok(Expression::new_range(start, end))
        } else {
            Ok(start)
        }
    }

    pub fn unary(&mut self) -> Result<Expression, SyntaxError> {
        // Check for spread operator before for loop
        if self.next_is(vec![scanner::TokenType::Spread]) {
            // Check if next token is For
            if self.current + 1 < self.input.len() {
                if let scanner::TokenType::For = self.input[self.current + 1].token_type {
                    return self.parse_for_loop_expression(true);
                }
            }
        }
        let expression = self.primary();
        expression
    }

    pub fn primary(&mut self) -> Result<Expression, SyntaxError> {
        let expr = match &self.input[self.current].token_type {
            scanner::TokenType::Integer(value) => {
                self.current += 1;
                Expression::Literal(Literal::Integer(*value))
            }
            scanner::TokenType::Float(value) => {
                self.current += 1;
                Expression::Literal(Literal::Float(*value))
            }
            scanner::TokenType::Boolean(value) => {
                self.current += 1;
                Expression::Literal(Literal::Boolean(*value))
            }
            scanner::TokenType::Identifier(value) => {
                self.current += 1;
                let identifier = Identifier::new(&value);
                let is_widget = self.next_is(vec![scanner::TokenType::LeftBracket]);
                if is_widget && !self.parsing_match_scrutinee {
                    // When parsing match scrutinee, `id {` is identifier + match arms, not a widget
                    let widget = self.parse_widget(identifier)?;
                    Expression::Widget(widget)
                } else {
                    Expression::Literal(Literal::Identifier(identifier))
                }
            }
            scanner::TokenType::LeftBracket => {
                let map = self.parse_map()?;
                Expression::Literal(Literal::Map(map))
            }
            scanner::TokenType::LeftSquareBracket => {
                let array = self.parse_array()?;
                Expression::Literal(Literal::Array(array))
            }
            scanner::TokenType::Fn => {
                let function = self.parse_function()?;
                Expression::Literal(Literal::Function(function))
            }
            scanner::TokenType::Match => return self.parse_match_expression(),
            scanner::TokenType::For => {
                let for_loop = self.parse_for_loop_expression(false)?;
                for_loop
            }
            scanner::TokenType::LeftParenthesis => {
                self.current += 1;
                let expression = self.expression()?;
                if self.next_is(vec![scanner::TokenType::RightParenthesis]) {
                    self.current += 1;
                    Expression::Grouping(Grouping {
                        value: Box::new(expression),
                    })
                } else {
                    let current_token = self.current().unwrap();
                    return Err(SyntaxError {
                        message: "Grouping is missing closing parenthesis".to_owned(),
                        line: current_token.line,
                        column: current_token.column,
                    });
                }
            }
            scanner::TokenType::String(value) => {
                self.current += 1;
                Expression::Literal(Literal::String(value.clone()))
            }
            _ => {
                let current_token = &self.input[self.current];
                return Err(SyntaxError {
                    message: "Invalid token while parsing unit expression".to_owned(),
                    line: current_token.line,
                    column: current_token.column,
                });
            }
        };

        self.parse_postfix(expr)
    }

    /// Parse postfix: member access (.prop), call (args), index ([expr]).
    fn parse_postfix(&mut self, mut expr: Expression) -> Result<Expression, SyntaxError> {
        loop {
            if self.next_is(vec![scanner::TokenType::Dot]) {
                self.consume_if(scanner::TokenType::Dot)?;
                let property = self.consume_if_identifier()?;
                expr = Expression::new_member_access(expr, property);
            } else if self.next_is(vec![scanner::TokenType::LeftParenthesis]) {
                let call = self.parse_call_with_callee(expr)?;
                expr = Expression::Call(call);
            } else if self.next_is(vec![scanner::TokenType::LeftSquareBracket]) {
                self.consume_if(scanner::TokenType::LeftSquareBracket)?;
                let index = self.expression()?;
                self.consume_if(scanner::TokenType::RightSquareBracket)?;
                expr = Expression::new_index_access(expr, index);
            } else {
                break;
            }
        }
        Ok(expr)
    }

    pub fn parse_string(&mut self) -> Result<String, SyntaxError> {
        let mut text = "".to_owned();
        while self.current().unwrap().token_type != scanner::TokenType::Quote {
            text.push_str(&self.parse_token_as_text());
            self.current += 1;
        }
        return Ok(text);
    }

    pub fn parse_function(&mut self) -> Result<Function, SyntaxError> {
        let mut function = Function::new();

        self.consume_if(scanner::TokenType::Fn)?;
        self.consume_if(scanner::TokenType::LeftParenthesis)?;

        while !self.next_is(vec![scanner::TokenType::RightParenthesis]) {
            let identifier = self.consume_if_identifier()?;
            self.consume_if(scanner::TokenType::Colon)?;
            let _arg_type = self.parse_type_identifier()?;
            function.arguments.push(identifier);
            if !self.next_is(vec![scanner::TokenType::RightParenthesis]) {
                self.consume_if(scanner::TokenType::Comma)?;
            }
        }

        self.consume_if(scanner::TokenType::RightParenthesis)?;
        function.return_type = self.parse_optional_type()?;
        self.consume_if(scanner::TokenType::LeftBracket)?;
        while self.current < self.input.len() {
            if let scanner::TokenType::RightBracket = self.input[self.current].token_type {
                self.consume_if(scanner::TokenType::RightBracket)?;
                return Ok(function);
            }
            let statement = self.parse_statement(false)?;
            function.body.statement_list.push(statement);
        }
        let current_token = if let Some(token) = self.current() {
            token
        } else {
            // If we're at the end, use the last token
            self.input.last().unwrap().clone()
        };
        return Err(SyntaxError {
            message: "Failed to parse function".to_owned(),
            line: current_token.line,
            column: current_token.column,
        });
    }

    /// Parse call arguments after the opening `(`; callee is already known.
    fn parse_call_with_callee(&mut self, callee: Expression) -> Result<Call, SyntaxError> {
        self.consume_if(scanner::TokenType::LeftParenthesis)?;
        let mut arguments = Vec::new();
        while !self.next_is(vec![scanner::TokenType::RightParenthesis]) {
            let argument = self.expression()?;
            arguments.push(argument);
            if !self.next_is(vec![scanner::TokenType::RightParenthesis]) {
                self.consume_if(scanner::TokenType::Comma)?;
            }
        }
        self.consume_if(scanner::TokenType::RightParenthesis)?;
        Ok(Call::new(callee, arguments))
    }

    pub fn parse_map(&mut self) -> Result<Map, SyntaxError> {
        self.consume_if(scanner::TokenType::LeftBracket)?;
        let mut map = Map::new();
        while self.current < self.input.len()
            && self.input[self.current].token_type != scanner::TokenType::RightBracket
        {
            let identifier = self.consume_if_identifier()?;
            self.consume_if(scanner::TokenType::Colon)?;
            let expression = self.expression()?;
            // Comma is optional before the closing bracket:
            // { a: 1 } and { a: 1, } are both valid.
            if self.current < self.input.len()
                && self.input[self.current].token_type != scanner::TokenType::RightBracket
            {
                self.consume_if(scanner::TokenType::Comma)?;
            }
            map.set(identifier, expression);
        }
        if self.current >= self.input.len() {
            let last = self.input.last().unwrap();
            return Err(SyntaxError {
                message: "Unterminated map literal".to_owned(),
                line: last.line,
                column: last.column,
            });
        }
        self.consume_if(scanner::TokenType::RightBracket)?;
        return Ok(map);
    }

    pub fn parse_array(&mut self) -> Result<Array, SyntaxError> {
        self.consume_if(scanner::TokenType::LeftSquareBracket)?;
        let mut array = Array::new();
        while !self.next_is(vec![scanner::TokenType::RightSquareBracket]) {
            // Check for spread for loop or spread expression
            if self.next_is(vec![scanner::TokenType::Spread]) {
                // Check if next token is For -> spread for loop
                if self.current + 1 < self.input.len() {
                    if let scanner::TokenType::For = self.input[self.current + 1].token_type {
                        let spread_for_loop = self.parse_for_loop_expression(true)?;
                        array.push(spread_for_loop);
                        if !self.next_is(vec![scanner::TokenType::RightSquareBracket]) {
                            self.consume_if(scanner::TokenType::Comma)?;
                        }
                        continue;
                    }
                }
                // Spread expression: ...expr
                self.consume_if(scanner::TokenType::Spread)?;
                let inner = self.expression()?;
                array.push(Expression::new_spread(inner));
                if !self.next_is(vec![scanner::TokenType::RightSquareBracket]) {
                    self.consume_if(scanner::TokenType::Comma)?;
                }
                continue;
            }
            let expression = self.expression()?;
            array.push(expression);
            if !self.next_is(vec![scanner::TokenType::RightSquareBracket]) {
                self.consume_if(scanner::TokenType::Comma)?;
            }
        }
        self.consume_if(scanner::TokenType::RightSquareBracket)?;
        return Ok(array);
    }

    pub fn parse_widget(&mut self, identifier: Identifier) -> Result<Widget, SyntaxError> {
        let mut widget = Widget::new(identifier);
        self.consume_if(scanner::TokenType::LeftBracket)?;
        while self.current < self.input.len()
            && self.input[self.current].token_type != scanner::TokenType::RightBracket
        {
            let key = self.consume_if_identifier()?;
            self.consume_if(scanner::TokenType::Colon)?;
            let value = self.expression()?;
            widget.set(key, value);
            if self.current < self.input.len()
                && self.input[self.current].token_type != scanner::TokenType::RightBracket
            {
                self.consume_if(scanner::TokenType::Comma)?;
            }
        }
        if self.current >= self.input.len() {
            let last = self.input.last().unwrap();
            return Err(SyntaxError {
                message: "Unterminated widget literal".to_owned(),
                line: last.line,
                column: last.column,
            });
        }
        self.consume_if(scanner::TokenType::RightBracket)?;
        return Ok(widget);
    }

    fn parse_token_as_text(&mut self) -> String {
        match self.current().unwrap().token_type {
            scanner::TokenType::Identifier(identifier) => identifier,
            scanner::TokenType::Not => "!".to_owned(),
            scanner::TokenType::Plus => "+".to_owned(),
            scanner::TokenType::Minus => "-".to_owned(),
            scanner::TokenType::Equal => "=".to_owned(),
            scanner::TokenType::EqualEqual => "==".to_owned(),
            _ => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::Scanner;

    fn parse(source: &str) -> Function {
        let mut scanner = Scanner::new(source.to_string());
        let tokens = scanner.scan();
        let mut parser = Parser::new(tokens);
        parser.parse().expect("parse error")
    }

    #[test]
    fn parse_let_declaration() {
        let module = parse("let x = 5;");
        assert_eq!(module.body.statement_list.len(), 1);
        match &module.body.statement_list[0] {
            Statement::Declare(decl) => {
                assert_eq!(decl.get_identifier_value(), "x");
            }
            other => panic!("Expected Declare, got {:?}", other),
        }
    }

    #[test]
    fn parse_function_definition() {
        let module = parse("let add = fn (a: int, b: int) { return a + b; };");
        assert_eq!(module.body.statement_list.len(), 1);
        match &module.body.statement_list[0] {
            Statement::Declare(decl) => {
                assert_eq!(decl.get_identifier_value(), "add");
            }
            other => panic!("Expected Declare, got {:?}", other),
        }
    }

    #[test]
    fn parse_if_else() {
        let module = parse("if true { return 1; } else { return 2; }");
        assert_eq!(module.body.statement_list.len(), 1);
        match &module.body.statement_list[0] {
            Statement::Conditional(_) => {}
            other => panic!("Expected Conditional, got {:?}", other),
        }
    }

    #[test]
    fn parse_state_declaration() {
        let module = parse("state count = 0;");
        assert_eq!(module.body.statement_list.len(), 1);
        match &module.body.statement_list[0] {
            Statement::DeclareState(state) => {
                assert_eq!(state.get_identifier_value(), "count");
            }
            other => panic!("Expected DeclareState, got {:?}", other),
        }
    }

    #[test]
    fn parse_widget_expression() {
        let module = parse("let w = Flex { width: 100 };");
        assert_eq!(module.body.statement_list.len(), 1);
    }

    #[test]
    fn parse_match_expression() {
        let module = parse(
            r#"let result = match x {
                1 => "one",
                2 => "two",
                _ => "other",
            };"#,
        );
        assert_eq!(module.body.statement_list.len(), 1);
    }

    #[test]
    fn parse_for_loop_statement() {
        let module = parse("for (i in 0..10) { log i; }");
        assert_eq!(module.body.statement_list.len(), 1);
        match &module.body.statement_list[0] {
            Statement::ForLoop(_) => {}
            other => panic!("Expected ForLoop, got {:?}", other),
        }
    }

    #[test]
    fn parse_import_statement() {
        let module = parse(r#"import "./button";"#);
        assert_eq!(module.body.statement_list.len(), 1);
        match &module.body.statement_list[0] {
            Statement::Import(_) => {}
            other => panic!("Expected Import, got {:?}", other),
        }
    }

    #[test]
    fn parse_array_literal() {
        let module = parse("let arr = [1, 2, 3];");
        assert_eq!(module.body.statement_list.len(), 1);
    }

    #[test]
    fn parse_binary_expression_precedence() {
        let module = parse("let x = 1 + 2 * 3;");
        assert_eq!(module.body.statement_list.len(), 1);
    }
}
