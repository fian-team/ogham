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
pub mod span;
mod statement;
mod syntax_error;
mod widget;

pub use {
    array::*, block::*, call::*, expression::*, function::*, identifier::*, literal::*, map::*,
    operator::*, span::*, statement::*, syntax_error::*, widget::*,
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

    // -- Span helpers -------------------------------------------------------

    /// Capture the start position from the current token.
    fn span_start(&self) -> (usize, usize) {
        if self.current < self.input.len() {
            let token = &self.input[self.current];
            (token.line, token.column)
        } else {
            (0, 0)
        }
    }

    /// Build a Span from a saved start position to the end of the previous token.
    fn span_from(&self, start: (usize, usize)) -> Span {
        if self.current > 0 {
            let prev = &self.input[self.current - 1];
            Span::new(start.0, start.1, prev.line, prev.column + prev.length)
        } else {
            Span::new(start.0, start.1, start.0, start.1)
        }
    }

    /// Build a span covering just the current token (before consuming it).
    fn current_token_span(&self) -> Span {
        let token = &self.input[self.current];
        Span::new(token.line, token.column, token.line, token.column + token.length)
    }

    // -- Token helpers ------------------------------------------------------

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
            scanner::TokenType::Modulo => Ok(Operator::Modulo),
            scanner::TokenType::Power => Ok(Operator::Power),
            scanner::TokenType::Not => Ok(Operator::Not),
            scanner::TokenType::NotEqual => Ok(Operator::NotEquals),
            scanner::TokenType::Or => Ok(Operator::Or),
            scanner::TokenType::And => Ok(Operator::And),
            _ => Err(SyntaxError {
                message: "Invalid operator".to_owned(),
                line,
                column,
            }),
        }
    }

    pub fn parse(&mut self) -> Result<Function, SyntaxError> {
        let start = self.span_start();
        let block = self.parse_block(true)?;
        self.module.body = block;
        self.module.span = self.span_from(start);
        Ok(self.module.clone())
    }

    pub fn parse_block(&mut self, allow_import: bool) -> Result<Block, SyntaxError> {
        let start = self.span_start();
        let mut block = Block::new();
        while self.current < self.input.len() {
            if let scanner::TokenType::EOF = self.input[self.current].token_type.clone() {
                break;
            }
            if let scanner::TokenType::RightBracket = self.input[self.current].token_type.clone() {
                break;
            }
            let statement = self.parse_statement(allow_import)?;
            block.statement_list.push(statement);
        }
        block.span = self.span_from(start);
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
            scanner::TokenType::Identifier(_) => self.parse_identifier_statement(),
            scanner::TokenType::Log => self.parse_log(),
            scanner::TokenType::For => self.parse_for_loop_statement(),
            _ => self.parse_expression_statement(),
        }
    }

    fn parse_import(&mut self) -> Result<Statement, SyntaxError> {
        let start = self.span_start();
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
        let span = self.span_from(start);
        Ok(Statement::new_import(names, path, span))
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
            let span = self.current_token_span();
            self.consume();
            Ok(Identifier::new(&identifier, span))
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

    fn is_at_block_end(&self) -> bool {
        self.current >= self.input.len()
            || matches!(
                self.input[self.current].token_type,
                scanner::TokenType::EOF | scanner::TokenType::RightBracket
            )
    }

    fn expression_to_statement(
        &mut self,
        expression: Expression,
        stmt_start: (usize, usize),
    ) -> Result<Statement, SyntaxError> {
        if self.next_is(vec![scanner::TokenType::Semicolon]) {
            self.consume_if(scanner::TokenType::Semicolon)?;
            let span = self.span_from(stmt_start);
            Ok(Statement::new_expression(expression, span))
        } else if self.is_at_block_end() {
            let span = self.span_from(stmt_start);
            Ok(Statement::new_return(Some(expression), span))
        } else {
            let span = self.span_from(stmt_start);
            Ok(Statement::new_expression(expression, span))
        }
    }

    fn parse_optional_type(&mut self) -> Result<Identifier, SyntaxError> {
        if self.next_is(vec![scanner::TokenType::Colon]) {
            self.consume_if(scanner::TokenType::Colon)?;
            self.parse_type_identifier()
        } else {
            Ok(Identifier::synthetic("infer"))
        }
    }

    /// Parses a type identifier, including postfix array syntax like `int[]` or `widget[][]`.
    fn parse_type_identifier(&mut self) -> Result<Identifier, SyntaxError> {
        let start = self.span_start();
        let base = self.consume_if_identifier()?;
        let mut type_str = base.get();

        while self.next_is(vec![scanner::TokenType::LeftSquareBracket]) {
            self.consume_if(scanner::TokenType::LeftSquareBracket)?;
            self.consume_if(scanner::TokenType::RightSquareBracket)?;
            type_str.push_str("[]");
        }

        let span = self.span_from(start);
        Ok(Identifier::new(&type_str, span))
    }

    fn parse_identifier_statement(&mut self) -> Result<Statement, SyntaxError> {
        let stmt_start = self.span_start();
        let identifier = self.consume_if_identifier()?;

        let next_token_type = if self.current < self.input.len() {
            Some(&self.input[self.current].token_type)
        } else {
            None
        };

        match next_token_type {
            Some(scanner::TokenType::LeftParenthesis) => {
                let expr = Expression::Literal(Literal::Identifier(identifier));
                let full_expr = self.parse_postfix(expr)?;
                self.expression_to_statement(full_expr, stmt_start)
            }
            Some(scanner::TokenType::LeftBracket) => {
                let widget = self.parse_widget(identifier)?;
                self.expression_to_statement(Expression::Widget(widget), stmt_start)
            }
            Some(scanner::TokenType::Equal) => self.parse_assign(identifier, stmt_start),
            _ => {
                // Allow postfix (`.prop`, `[idx]`, etc.) on a bare identifier
                // appearing as a statement/trailing expression — e.g.
                //   state m = mutation("foo");
                //   m.status        // trailing expression
                let expr = Expression::Literal(Literal::Identifier(identifier));
                let full_expr = self.parse_postfix(expr)?;
                self.expression_to_statement(full_expr, stmt_start)
            }
        }
    }

    fn parse_expression_statement(&mut self) -> Result<Statement, SyntaxError> {
        let start = self.span_start();
        let expression = self.expression()?;
        self.expression_to_statement(expression, start)
    }

    pub fn parse_conditional(&mut self) -> Result<Statement, SyntaxError> {
        let start = self.span_start();
        self.consume_if(scanner::TokenType::If)?;
        let condition = self.expression()?;
        self.consume_if(scanner::TokenType::LeftBracket)?;
        let block = self.parse_block(false)?;
        self.consume_if(scanner::TokenType::RightBracket)?;

        let mut branches = vec![(condition, block)];
        let mut else_block = None;

        while self.next_is(vec![scanner::TokenType::Else]) {
            if self.current + 1 < self.input.len() {
                if let scanner::TokenType::If = self.input[self.current + 1].token_type {
                    self.consume_if(scanner::TokenType::Else)?;
                    self.consume_if(scanner::TokenType::If)?;
                    let else_if_condition = self.expression()?;
                    self.consume_if(scanner::TokenType::LeftBracket)?;
                    let else_if_block = self.parse_block(false)?;
                    self.consume_if(scanner::TokenType::RightBracket)?;
                    branches.push((else_if_condition, else_if_block));
                } else {
                    self.consume_if(scanner::TokenType::Else)?;
                    self.consume_if(scanner::TokenType::LeftBracket)?;
                    else_block = Some(self.parse_block(false)?);
                    self.consume_if(scanner::TokenType::RightBracket)?;
                    break;
                }
            } else {
                self.consume_if(scanner::TokenType::Else)?;
                self.consume_if(scanner::TokenType::LeftBracket)?;
                else_block = Some(self.parse_block(false)?);
                self.consume_if(scanner::TokenType::RightBracket)?;
                break;
            }
        }

        let span = self.span_from(start);
        Ok(Statement::new_conditional(branches, else_block, span))
    }

    pub fn parse_return(&mut self) -> Result<Statement, SyntaxError> {
        let start = self.span_start();
        self.consume_if(scanner::TokenType::Return)?;
        if self.next_is(vec![scanner::TokenType::Semicolon]) {
            self.consume_if(scanner::TokenType::Semicolon)?;
            let span = self.span_from(start);
            return Ok(Statement::new_return(None, span));
        }
        let expression = self.expression()?;
        self.consume_if(scanner::TokenType::Semicolon)?;
        let span = self.span_from(start);
        Ok(Statement::new_return(Some(expression), span))
    }

    pub fn parse_let(&mut self) -> Result<Statement, SyntaxError> {
        let start = self.span_start();
        self.consume_if(scanner::TokenType::Let)?;
        let (variable_name, _variable_type, variable_value) =
            self.parse_remainder_of_declaration()?;
        let span = self.span_from(start);
        Ok(Statement::new_declare(&variable_name, variable_value, span))
    }

    pub fn parse_state(&mut self) -> Result<Statement, SyntaxError> {
        let start = self.span_start();
        self.consume_if(scanner::TokenType::State)?;
        let (variable_name, _variable_type, variable_value) =
            self.parse_remainder_of_declaration()?;
        let span = self.span_from(start);
        Ok(Statement::new_declare_state(
            &variable_name,
            variable_value,
            span,
        ))
    }

    fn parse_remainder_of_declaration(
        &mut self,
    ) -> Result<(Identifier, Identifier, Expression), SyntaxError> {
        let identifier = self.consume_if_identifier()?;
        let identifier_type = self.parse_optional_type()?;
        self.consume_if(scanner::TokenType::Equal)?;
        let expression = self.expression()?;
        self.consume_if(scanner::TokenType::Semicolon)?;
        Ok((identifier, identifier_type, expression))
    }

    pub fn parse_assign(
        &mut self,
        identifier: Identifier,
        stmt_start: (usize, usize),
    ) -> Result<Statement, SyntaxError> {
        self.consume_if(scanner::TokenType::Equal)?;
        let expression = self.expression()?;
        self.consume_if(scanner::TokenType::Semicolon)?;
        let span = self.span_from(stmt_start);
        Ok(Statement::new_assign(&identifier, expression, span))
    }

    pub fn parse_log(&mut self) -> Result<Statement, SyntaxError> {
        let start = self.span_start();
        self.consume_if(scanner::TokenType::Log)?;
        let expression = self.expression()?;
        self.consume_if(scanner::TokenType::Semicolon)?;
        let span = self.span_from(start);
        Ok(Statement::new_log(expression, span))
    }

    pub fn parse_for_loop_statement(&mut self) -> Result<Statement, SyntaxError> {
        let start = self.span_start();
        self.consume_if(scanner::TokenType::For)?;
        self.consume_if(scanner::TokenType::LeftParenthesis)?;
        let variable = self.consume_if_identifier()?;
        self.consume_if(scanner::TokenType::In)?;
        let range_start = self.factor()?;
        self.consume_if(scanner::TokenType::Range)?;
        let range_end = self.factor()?;
        self.consume_if(scanner::TokenType::RightParenthesis)?;
        self.consume_if(scanner::TokenType::LeftBracket)?;
        let body = self.parse_block(false)?;
        self.consume_if(scanner::TokenType::RightBracket)?;
        let span = self.span_from(start);
        Ok(Statement::new_for_loop(
            variable,
            range_start,
            range_end,
            body,
            span,
        ))
    }

    fn parse_for_loop_expression(&mut self, is_spread: bool) -> Result<Expression, SyntaxError> {
        let start = self.span_start();
        if is_spread {
            self.consume_if(scanner::TokenType::Spread)?;
        }
        self.consume_if(scanner::TokenType::For)?;
        self.consume_if(scanner::TokenType::LeftParenthesis)?;
        let variable = self.consume_if_identifier()?;
        self.consume_if(scanner::TokenType::In)?;
        let range_start = self.factor()?;
        self.consume_if(scanner::TokenType::Range)?;
        let range_end = self.factor()?;
        self.consume_if(scanner::TokenType::RightParenthesis)?;
        self.consume_if(scanner::TokenType::LeftBracket)?;
        let body = self.parse_block(false)?;
        self.consume_if(scanner::TokenType::RightBracket)?;
        let span = self.span_from(start);
        if is_spread {
            Ok(Expression::new_spread_for_loop(
                variable,
                range_start,
                range_end,
                body,
                span,
            ))
        } else {
            Ok(Expression::new_for_loop(
                variable,
                range_start,
                range_end,
                body,
                span,
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
        let span = self.current_token_span();
        let expr = match &current_token.token_type {
            scanner::TokenType::Integer(value) => {
                self.current += 1;
                Expression::Literal(Literal::Integer(*value, span))
            }
            scanner::TokenType::Float(value) => {
                self.current += 1;
                Expression::Literal(Literal::Float(*value, span))
            }
            scanner::TokenType::Boolean(value) => {
                self.current += 1;
                Expression::Literal(Literal::Boolean(*value, span))
            }
            scanner::TokenType::String(value) => {
                self.current += 1;
                Expression::Literal(Literal::String(value.clone(), span))
            }
            scanner::TokenType::Identifier(value) => {
                self.current += 1;
                Expression::Literal(Literal::Identifier(Identifier::new(value, span)))
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
        let start = self.span_start();
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
                let expr_span = expr.span();
                let mut block = Block::new();
                block.span = expr_span;
                block
                    .statement_list
                    .push(Statement::new_return(Some(expr), expr_span));
                block
            };

            arms.push((pattern, body));

            if self.next_is(vec![scanner::TokenType::Comma]) {
                self.consume_if(scanner::TokenType::Comma)?;
            }
        }

        self.consume_if(scanner::TokenType::RightBracket)?;
        let span = self.span_from(start);
        Ok(Expression::new_match(scrutinee, arms, span))
    }

    pub fn expression(&mut self) -> Result<Expression, SyntaxError> {
        self.logical_or()
    }

    pub fn logical_or(&mut self) -> Result<Expression, SyntaxError> {
        let mut expression = self.logical_and()?;
        while self.next_is(vec![scanner::TokenType::Or]) {
            let left_start = expression.span();
            let operator = self.get_current_as_operator()?;
            let right = self.logical_and()?;
            let span = Span::merge(&left_start, &right.span());
            expression = Expression::new_binary(expression, operator, right, span);
        }
        Ok(expression)
    }

    pub fn logical_and(&mut self) -> Result<Expression, SyntaxError> {
        let mut expression = self.equality()?;
        while self.next_is(vec![scanner::TokenType::And]) {
            let left_start = expression.span();
            let operator = self.get_current_as_operator()?;
            let right = self.equality()?;
            let span = Span::merge(&left_start, &right.span());
            expression = Expression::new_binary(expression, operator, right, span);
        }
        Ok(expression)
    }

    pub fn equality(&mut self) -> Result<Expression, SyntaxError> {
        let mut expression = self.comparison()?;
        while self.next_is(vec![
            scanner::TokenType::EqualEqual,
            scanner::TokenType::NotEqual,
        ]) {
            let left_start = expression.span();
            let operator = self.get_current_as_operator()?;
            let right = self.comparison()?;
            let span = Span::merge(&left_start, &right.span());
            expression = Expression::new_binary(expression, operator, right, span);
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
            let left_start = expression.span();
            let operator = self.get_current_as_operator()?;
            let right = self.term()?;
            let span = Span::merge(&left_start, &right.span());
            expression = Expression::new_binary(expression, operator, right, span);
        }
        Ok(expression)
    }

    pub fn term(&mut self) -> Result<Expression, SyntaxError> {
        let mut expression = self.range()?;
        while self.next_is(vec![scanner::TokenType::Plus, scanner::TokenType::Minus]) {
            let left_start = expression.span();
            let operator = self.get_current_as_operator()?;
            let right = self.range()?;
            let span = Span::merge(&left_start, &right.span());
            expression = Expression::new_binary(expression, operator, right, span);
        }
        Ok(expression)
    }

    pub fn factor(&mut self) -> Result<Expression, SyntaxError> {
        let mut expression = self.exponent()?;
        while self.next_is(vec![
            scanner::TokenType::Multiply,
            scanner::TokenType::Divide,
            scanner::TokenType::Modulo,
        ]) {
            let left_start = expression.span();
            let operator = self.get_current_as_operator()?;
            let right = self.exponent()?;
            let span = Span::merge(&left_start, &right.span());
            expression = Expression::new_binary(expression, operator, right, span);
        }
        Ok(expression)
    }

    pub fn exponent(&mut self) -> Result<Expression, SyntaxError> {
        let base = self.unary()?;
        if self.next_is(vec![scanner::TokenType::Power]) {
            let left_start = base.span();
            let operator = self.get_current_as_operator()?;
            let exp = self.exponent()?;
            let span = Span::merge(&left_start, &exp.span());
            Ok(Expression::new_binary(base, operator, exp, span))
        } else {
            Ok(base)
        }
    }

    pub fn range(&mut self) -> Result<Expression, SyntaxError> {
        let start = self.factor()?;
        if self.next_is(vec![scanner::TokenType::Range]) {
            self.consume();
            let end = self.factor()?;
            let span = Span::merge(&start.span(), &end.span());
            Ok(Expression::new_range(start, end, span))
        } else {
            Ok(start)
        }
    }

    pub fn unary(&mut self) -> Result<Expression, SyntaxError> {
        if self.next_is(vec![scanner::TokenType::Spread]) {
            if self.current + 1 < self.input.len() {
                if let scanner::TokenType::For = self.input[self.current + 1].token_type {
                    return self.parse_for_loop_expression(true);
                }
            }
        }
        if self.next_is(vec![scanner::TokenType::Increment]) {
            let start = self.span_start();
            let token = self.current().unwrap();
            let line = token.line;
            let column = token.column;
            self.consume();
            let ident = self.consume_if_identifier().map_err(|_| SyntaxError {
                message: "Prefix ++ can only be applied to a variable".to_owned(),
                line,
                column,
            })?;
            let span = self.span_from(start);
            return Ok(Expression::PrefixIncrement(IncrementExpression {
                identifier: ident,
                span,
            }));
        }
        if self.next_is(vec![scanner::TokenType::Minus]) {
            let start = self.span_start();
            self.consume();
            let operand = self.unary()?;
            let span = self.span_from(start);
            return Ok(Expression::new_unary(Operator::Minus, operand, span));
        }
        if self.next_is(vec![scanner::TokenType::Not]) {
            let start = self.span_start();
            self.consume();
            let operand = self.unary()?;
            let span = self.span_from(start);
            return Ok(Expression::new_unary(Operator::Not, operand, span));
        }
        self.primary()
    }

    pub fn primary(&mut self) -> Result<Expression, SyntaxError> {
        let expr = match &self.input[self.current].token_type {
            scanner::TokenType::Integer(value) => {
                let span = self.current_token_span();
                self.current += 1;
                Expression::Literal(Literal::Integer(*value, span))
            }
            scanner::TokenType::Float(value) => {
                let span = self.current_token_span();
                self.current += 1;
                Expression::Literal(Literal::Float(*value, span))
            }
            scanner::TokenType::Boolean(value) => {
                let span = self.current_token_span();
                self.current += 1;
                Expression::Literal(Literal::Boolean(*value, span))
            }
            scanner::TokenType::Identifier(value) => {
                let span = self.current_token_span();
                self.current += 1;
                let identifier = Identifier::new(value, span);
                let is_widget = self.next_is(vec![scanner::TokenType::LeftBracket]);
                if is_widget && !self.parsing_match_scrutinee {
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
                return self.parse_for_loop_expression(false);
            }
            scanner::TokenType::LeftParenthesis => {
                let start = self.span_start();
                self.current += 1;
                let expression = self.expression()?;
                if self.next_is(vec![scanner::TokenType::RightParenthesis]) {
                    self.current += 1;
                    let span = self.span_from(start);
                    Expression::Grouping(Grouping {
                        value: Box::new(expression),
                        span,
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
                let span = self.current_token_span();
                self.current += 1;
                Expression::Literal(Literal::String(value.clone(), span))
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
                let start_span = expr.span();
                self.consume_if(scanner::TokenType::Dot)?;
                let property = self.consume_if_identifier()?;
                let span = Span::merge(&start_span, &property.span);
                expr = Expression::new_member_access(expr, property, span);
            } else if self.next_is(vec![scanner::TokenType::LeftParenthesis]) {
                let callee_span = expr.span();
                let call = self.parse_call_with_callee(expr, callee_span)?;
                expr = Expression::Call(call);
            } else if self.next_is(vec![scanner::TokenType::LeftSquareBracket]) {
                let start_span = expr.span();
                self.consume_if(scanner::TokenType::LeftSquareBracket)?;
                let index = self.expression()?;
                self.consume_if(scanner::TokenType::RightSquareBracket)?;
                let span = self.span_from((start_span.start_line, start_span.start_column));
                expr = Expression::new_index_access(expr, index, span);
            } else if self.next_is(vec![scanner::TokenType::Increment]) {
                if let Expression::Literal(Literal::Identifier(ref ident)) = expr {
                    let ident = ident.clone();
                    let start_span = ident.span;
                    self.consume();
                    let span = self.span_from((start_span.start_line, start_span.start_column));
                    expr = Expression::PostfixIncrement(IncrementExpression {
                        identifier: ident,
                        span,
                    });
                } else {
                    break;
                }
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
        Ok(text)
    }

    pub fn parse_function(&mut self) -> Result<Function, SyntaxError> {
        let start = self.span_start();
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
                function.span = self.span_from(start);
                return Ok(function);
            }
            let statement = self.parse_statement(false)?;
            function.body.statement_list.push(statement);
        }
        let current_token = if let Some(token) = self.current() {
            token
        } else {
            self.input.last().unwrap().clone()
        };
        Err(SyntaxError {
            message: "Failed to parse function".to_owned(),
            line: current_token.line,
            column: current_token.column,
        })
    }

    /// Parse call arguments after the opening `(`; callee is already known.
    fn parse_call_with_callee(
        &mut self,
        callee: Expression,
        callee_span: Span,
    ) -> Result<Call, SyntaxError> {
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
        let span =
            self.span_from((callee_span.start_line, callee_span.start_column));
        Ok(Call::new(callee, arguments, span))
    }

    pub fn parse_map(&mut self) -> Result<Map, SyntaxError> {
        let start = self.span_start();
        self.consume_if(scanner::TokenType::LeftBracket)?;
        let mut map = Map::new();
        while self.current < self.input.len()
            && self.input[self.current].token_type != scanner::TokenType::RightBracket
        {
            let identifier = self.consume_if_identifier()?;
            self.consume_if(scanner::TokenType::Colon)?;
            let expression = self.expression()?;
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
        map.span = self.span_from(start);
        Ok(map)
    }

    pub fn parse_array(&mut self) -> Result<Array, SyntaxError> {
        let start = self.span_start();
        self.consume_if(scanner::TokenType::LeftSquareBracket)?;
        let mut array = Array::new();
        while !self.next_is(vec![scanner::TokenType::RightSquareBracket]) {
            if self.next_is(vec![scanner::TokenType::Spread]) {
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
                let spread_start = self.span_start();
                self.consume_if(scanner::TokenType::Spread)?;
                let inner = self.expression()?;
                let spread_span = self.span_from(spread_start);
                array.push(Expression::new_spread(inner, spread_span));
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
        array.span = self.span_from(start);
        Ok(array)
    }

    pub fn parse_widget(&mut self, identifier: Identifier) -> Result<Widget, SyntaxError> {
        let start = (identifier.span.start_line, identifier.span.start_column);
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
        widget.span = self.span_from(start);
        Ok(widget)
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

    #[test]
    fn parse_unary_negation_integer() {
        let module = parse("let x = -5;");
        let expr = match &module.body.statement_list[0] {
            Statement::Declare(decl) => decl.get_value(),
            other => panic!("Expected Declare, got {:?}", other),
        };
        match expr {
            Expression::Unary(unary) => {
                assert_eq!(unary.operator, Operator::Minus);
                assert!(matches!(*unary.value, Expression::Literal(Literal::Integer(5, _))));
            }
            other => panic!("Expected Unary, got {:?}", other),
        }
    }

    #[test]
    fn parse_unary_not() {
        let module = parse("let x = !true;");
        let expr = match &module.body.statement_list[0] {
            Statement::Declare(decl) => decl.get_value(),
            other => panic!("Expected Declare, got {:?}", other),
        };
        match expr {
            Expression::Unary(unary) => {
                assert_eq!(unary.operator, Operator::Not);
                assert!(matches!(*unary.value, Expression::Literal(Literal::Boolean(true, _))));
            }
            other => panic!("Expected Unary, got {:?}", other),
        }
    }

    #[test]
    fn parse_double_negation() {
        let module = parse("let x = --5;");
        let expr = match &module.body.statement_list[0] {
            Statement::Declare(decl) => decl.get_value(),
            other => panic!("Expected Declare, got {:?}", other),
        };
        match &expr {
            Expression::Unary(outer) => {
                assert_eq!(outer.operator, Operator::Minus);
                match outer.value.as_ref() {
                    Expression::Unary(inner) => {
                        assert_eq!(inner.operator, Operator::Minus);
                        assert!(matches!(*inner.value, Expression::Literal(Literal::Integer(5, _))));
                    }
                    other => panic!("Expected inner Unary, got {:?}", other),
                }
            }
            other => panic!("Expected outer Unary, got {:?}", other),
        }
    }

    #[test]
    fn parse_binary_minus_with_unary_minus() {
        let module = parse("let x = 0 - -5;");
        let expr = match &module.body.statement_list[0] {
            Statement::Declare(decl) => decl.get_value(),
            other => panic!("Expected Declare, got {:?}", other),
        };
        match &expr {
            Expression::Binary(binary) => {
                assert_eq!(binary.operator, Operator::Minus);
                assert!(matches!(*binary.left, Expression::Literal(Literal::Integer(0, _))));
                match binary.right.as_ref() {
                    Expression::Unary(unary) => {
                        assert_eq!(unary.operator, Operator::Minus);
                        assert!(matches!(*unary.value, Expression::Literal(Literal::Integer(5, _))));
                    }
                    other => panic!("Expected Unary on right, got {:?}", other),
                }
            }
            other => panic!("Expected Binary, got {:?}", other),
        }
    }

    #[test]
    fn parse_unary_negation_float() {
        let module = parse("let x = -3.14;");
        let expr = match &module.body.statement_list[0] {
            Statement::Declare(decl) => decl.get_value(),
            other => panic!("Expected Declare, got {:?}", other),
        };
        match expr {
            Expression::Unary(unary) => {
                assert_eq!(unary.operator, Operator::Minus);
                match *unary.value {
                    Expression::Literal(Literal::Float(f, _)) => {
                        assert!((f - 3.14).abs() < f64::EPSILON);
                    }
                    ref other => panic!("Expected Float literal, got {:?}", other),
                }
            }
            other => panic!("Expected Unary, got {:?}", other),
        }
    }

    #[test]
    fn parse_logical_or() {
        let module = parse("let x = a || b;");
        let expr = match &module.body.statement_list[0] {
            Statement::Declare(decl) => decl.get_value(),
            other => panic!("Expected Declare, got {:?}", other),
        };
        match &expr {
            Expression::Binary(binary) => {
                assert_eq!(binary.operator, Operator::Or);
            }
            other => panic!("Expected Binary Or, got {:?}", other),
        }
    }

    #[test]
    fn parse_logical_and() {
        let module = parse("let x = a && b;");
        let expr = match &module.body.statement_list[0] {
            Statement::Declare(decl) => decl.get_value(),
            other => panic!("Expected Declare, got {:?}", other),
        };
        match &expr {
            Expression::Binary(binary) => {
                assert_eq!(binary.operator, Operator::And);
            }
            other => panic!("Expected Binary And, got {:?}", other),
        }
    }

    #[test]
    fn parse_logical_precedence_and_binds_tighter_than_or() {
        let module = parse("let x = a || b && c;");
        let expr = match &module.body.statement_list[0] {
            Statement::Declare(decl) => decl.get_value(),
            other => panic!("Expected Declare, got {:?}", other),
        };
        match &expr {
            Expression::Binary(binary) => {
                assert_eq!(binary.operator, Operator::Or);
                match binary.right.as_ref() {
                    Expression::Binary(inner) => {
                        assert_eq!(inner.operator, Operator::And);
                    }
                    other => panic!("Expected inner Binary And, got {:?}", other),
                }
            }
            other => panic!("Expected Binary Or, got {:?}", other),
        }
    }

    #[test]
    fn parse_logical_with_grouping() {
        let module = parse("let x = (a || b) && c;");
        let expr = match &module.body.statement_list[0] {
            Statement::Declare(decl) => decl.get_value(),
            other => panic!("Expected Declare, got {:?}", other),
        };
        match &expr {
            Expression::Binary(binary) => {
                assert_eq!(binary.operator, Operator::And);
                match binary.left.as_ref() {
                    Expression::Grouping(g) => match g.value.as_ref() {
                        Expression::Binary(inner) => {
                            assert_eq!(inner.operator, Operator::Or);
                        }
                        other => panic!("Expected inner Binary Or, got {:?}", other),
                    },
                    other => panic!("Expected Grouping, got {:?}", other),
                }
            }
            other => panic!("Expected Binary And, got {:?}", other),
        }
    }
}
