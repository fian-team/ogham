use ogham::parser::*;
use ogham::scanner::{Token, TokenType};
use tower_lsp::lsp_types::*;

pub const TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::KEYWORD,   // 0
    SemanticTokenType::VARIABLE,  // 1
    SemanticTokenType::FUNCTION,  // 2
    SemanticTokenType::PARAMETER, // 3
    SemanticTokenType::TYPE,      // 4
    SemanticTokenType::PROPERTY,  // 5
    SemanticTokenType::NUMBER,    // 6
    SemanticTokenType::STRING,    // 7
    SemanticTokenType::OPERATOR,  // 8
];

pub const TOKEN_MODIFIERS: &[SemanticTokenModifier] = &[
    SemanticTokenModifier::DECLARATION, // bit 0
    SemanticTokenModifier::READONLY,    // bit 1
];

const TT_KEYWORD: u32 = 0;
const TT_VARIABLE: u32 = 1;
const TT_FUNCTION: u32 = 2;
const TT_PARAMETER: u32 = 3;
const TT_TYPE: u32 = 4;
const TT_PROPERTY: u32 = 5;
const TT_NUMBER: u32 = 6;
const TT_STRING: u32 = 7;
const TT_OPERATOR: u32 = 8;

const MOD_DECLARATION: u32 = 1 << 0;
const MOD_READONLY: u32 = 1 << 1;

/// A raw semantic token before delta-encoding.
#[derive(Clone)]
struct RawToken {
    line: u32,
    col: u32,
    length: u32,
    token_type: u32,
    modifiers: u32,
}

/// Build the full list of semantic tokens for a document.
pub fn build_semantic_tokens(tokens: &[Token], ast: Option<&Function>) -> Vec<SemanticToken> {
    let mut raw: Vec<RawToken> = Vec::new();

    collect_from_token_stream(tokens, &mut raw);

    if let Some(module) = ast {
        let mut ctx = AstContext::new();
        collect_from_function(module, &mut raw, &mut ctx);
    }

    raw.sort_by(|a, b| a.line.cmp(&b.line).then(a.col.cmp(&b.col)));
    raw.dedup_by(|b, a| a.line == b.line && a.col == b.col);

    delta_encode(&raw)
}

fn collect_from_token_stream(tokens: &[Token], out: &mut Vec<RawToken>) {
    for token in tokens {
        let (token_type, length) = match &token.token_type {
            TokenType::Let
            | TokenType::State
            | TokenType::If
            | TokenType::Else
            | TokenType::Return
            | TokenType::Log
            | TokenType::Fn
            | TokenType::For
            | TokenType::In
            | TokenType::Match
            | TokenType::Import
            | TokenType::From => (TT_KEYWORD, token.length as u32),
            TokenType::Plus
            | TokenType::Minus
            | TokenType::Multiply
            | TokenType::Divide
            | TokenType::Modulo
            | TokenType::Power
            | TokenType::EqualEqual
            | TokenType::NotEqual
            | TokenType::GreaterThan
            | TokenType::GreaterThanOrEqualTo
            | TokenType::LessThan
            | TokenType::LessThanOrEqualTo
            | TokenType::Not
            | TokenType::And
            | TokenType::Or
            | TokenType::Equal
            | TokenType::FatArrow
            | TokenType::Range
            | TokenType::Spread
            | TokenType::Increment => (TT_OPERATOR, token.length as u32),
            _ => continue,
        };
        out.push(RawToken {
            line: token.line.saturating_sub(1) as u32,
            col: token.column.saturating_sub(1) as u32,
            length,
            token_type,
            modifiers: 0,
        });
    }
}

/// Tracks which identifiers are known to be functions vs. state variables.
struct AstContext {
    functions: Vec<String>,
    state_vars: Vec<String>,
    params: Vec<String>,
}

impl AstContext {
    fn new() -> Self {
        Self {
            functions: Vec::new(),
            state_vars: Vec::new(),
            params: Vec::new(),
        }
    }
}

fn collect_from_function(func: &Function, out: &mut Vec<RawToken>, ctx: &mut AstContext) {
    let saved_params = ctx.params.clone();
    for arg in &func.arguments {
        push_ident(out, arg, TT_PARAMETER, MOD_DECLARATION);
        ctx.params.push(arg.get());
    }
    collect_from_block(&func.body, out, ctx);
    ctx.params = saved_params;
}

fn collect_from_block(block: &Block, out: &mut Vec<RawToken>, ctx: &mut AstContext) {
    for stmt in &block.statement_list {
        collect_from_statement(stmt, out, ctx);
    }
}

fn collect_from_statement(stmt: &Statement, out: &mut Vec<RawToken>, ctx: &mut AstContext) {
    match stmt {
        Statement::Declare(decl) => {
            let is_fn = matches!(&decl.value, Expression::Literal(Literal::Function(_)));
            if is_fn {
                push_ident(out, &decl.identifier, TT_FUNCTION, MOD_DECLARATION);
                ctx.functions.push(decl.identifier.get());
            } else {
                push_ident(out, &decl.identifier, TT_VARIABLE, MOD_DECLARATION);
            }
            collect_from_expression(&decl.value, out, ctx);
        }
        Statement::DeclareState(decl) => {
            push_ident(
                out,
                &decl.identifier,
                TT_VARIABLE,
                MOD_DECLARATION | MOD_READONLY,
            );
            ctx.state_vars.push(decl.identifier.get());
            collect_from_expression(&decl.value, out, ctx);
        }
        Statement::Assign(assign) => {
            push_ident_ref(out, &assign.identifier, ctx);
            collect_from_expression(&assign.value, out, ctx);
        }
        Statement::Expression(expr_stmt) => {
            collect_from_expression(&expr_stmt.value, out, ctx);
        }
        Statement::Return(ret) => {
            if let Some(expr) = &ret.value {
                collect_from_expression(expr, out, ctx);
            }
        }
        Statement::Conditional(cond) => {
            for (condition, block) in &cond.branches {
                collect_from_expression(condition, out, ctx);
                collect_from_block(block, out, ctx);
            }
            if let Some(else_block) = &cond.else_block {
                collect_from_block(else_block, out, ctx);
            }
        }
        Statement::Log(log) => {
            collect_from_expression(&log.value, out, ctx);
        }
        Statement::ForLoop(for_loop) => {
            push_ident(out, &for_loop.variable, TT_VARIABLE, MOD_DECLARATION);
            collect_from_expression(&for_loop.range_start, out, ctx);
            collect_from_expression(&for_loop.range_end, out, ctx);
            collect_from_block(&for_loop.body, out, ctx);
        }
        Statement::Import(_) => {}
        // Typed-bindings declarations: M3 will add proper semantic
        // highlighting for record/host_state/events keywords, type
        // refs, and field names. For now they receive no special
        // tokens (they'll appear unhighlighted).
        Statement::RecordDeclaration(_)
        | Statement::HostStateDeclaration(_)
        | Statement::EventsDeclaration(_) => {}
    }
}

fn collect_from_expression(expr: &Expression, out: &mut Vec<RawToken>, ctx: &mut AstContext) {
    match expr {
        Expression::Literal(lit) => collect_from_literal(lit, out, ctx),
        Expression::Unary(u) => {
            collect_from_expression(&u.value, out, ctx);
        }
        Expression::Binary(b) => {
            collect_from_expression(&b.left, out, ctx);
            collect_from_expression(&b.right, out, ctx);
        }
        Expression::Grouping(g) => {
            collect_from_expression(&g.value, out, ctx);
        }
        Expression::Widget(w) => {
            push_ident(out, &w.identifier, TT_TYPE, 0);
            for (key, value) in &w.properties {
                push_ident(out, key, TT_PROPERTY, 0);
                collect_from_expression(value, out, ctx);
            }
        }
        Expression::MemberAccess(m) => {
            collect_from_expression(&m.object, out, ctx);
            push_ident(out, &m.property, TT_PROPERTY, 0);
        }
        Expression::Call(c) => {
            collect_from_expression(&c.callee, out, ctx);
            for arg in &c.arguments {
                collect_from_expression(arg, out, ctx);
            }
        }
        Expression::IndexAccess(i) => {
            collect_from_expression(&i.object, out, ctx);
            collect_from_expression(&i.index, out, ctx);
        }
        Expression::Range(r) => {
            collect_from_expression(&r.start, out, ctx);
            collect_from_expression(&r.end, out, ctx);
        }
        Expression::ForLoop(f) | Expression::SpreadForLoop(f) => {
            push_ident(out, &f.variable, TT_VARIABLE, MOD_DECLARATION);
            collect_from_expression(&f.range_start, out, ctx);
            collect_from_expression(&f.range_end, out, ctx);
            collect_from_block(&f.body, out, ctx);
        }
        Expression::Spread(s) => {
            collect_from_expression(&s.inner, out, ctx);
        }
        Expression::Match(m) => {
            collect_from_expression(&m.scrutinee, out, ctx);
            for (pattern, block) in &m.arms {
                collect_from_expression(pattern, out, ctx);
                collect_from_block(block, out, ctx);
            }
        }
        Expression::PrefixIncrement(inc) | Expression::PostfixIncrement(inc) => {
            push_ident_ref(out, &inc.identifier, ctx);
        }
    }
}

fn collect_from_literal(lit: &Literal, out: &mut Vec<RawToken>, ctx: &mut AstContext) {
    match lit {
        Literal::Integer(_, span) => {
            push_span(out, span, TT_NUMBER, 0);
        }
        Literal::Float(_, span) => {
            push_span(out, span, TT_NUMBER, 0);
        }
        Literal::Boolean(_, span) => {
            push_span(out, span, TT_KEYWORD, 0);
        }
        Literal::String(_, span) => {
            push_span(out, span, TT_STRING, 0);
        }
        Literal::Identifier(ident) => {
            push_ident_ref(out, ident, ctx);
        }
        Literal::Function(func) => {
            collect_from_function(func, out, ctx);
        }
        Literal::Map(map) => {
            for (key, value) in &map.properties {
                push_ident(out, key, TT_PROPERTY, 0);
                collect_from_expression(value, out, ctx);
            }
        }
        Literal::Array(array) => {
            for elem in &array.elements {
                collect_from_expression(elem, out, ctx);
            }
        }
    }
}

fn push_ident(out: &mut Vec<RawToken>, ident: &Identifier, token_type: u32, modifiers: u32) {
    let span = &ident.span;
    if span.start_line == 0 {
        return;
    }
    out.push(RawToken {
        line: span.start_line.saturating_sub(1) as u32,
        col: span.start_column.saturating_sub(1) as u32,
        length: ident.get().len() as u32,
        token_type,
        modifiers,
    });
}

/// Push an identifier reference, classifying it based on what we've seen declared.
fn push_ident_ref(out: &mut Vec<RawToken>, ident: &Identifier, ctx: &AstContext) {
    let name = ident.get();
    let (tt, mods) = if ctx.params.contains(&name) {
        (TT_PARAMETER, 0)
    } else if ctx.functions.contains(&name) {
        (TT_FUNCTION, 0)
    } else if ctx.state_vars.contains(&name) {
        (TT_VARIABLE, MOD_READONLY)
    } else {
        (TT_VARIABLE, 0)
    };
    push_ident(out, ident, tt, mods);
}

fn push_span(out: &mut Vec<RawToken>, span: &Span, token_type: u32, modifiers: u32) {
    if span.start_line == 0 {
        return;
    }
    let length = if span.start_line == span.end_line {
        (span.end_column - span.start_column) as u32
    } else {
        1
    };
    out.push(RawToken {
        line: span.start_line.saturating_sub(1) as u32,
        col: span.start_column.saturating_sub(1) as u32,
        length,
        token_type,
        modifiers,
    });
}

fn delta_encode(raw: &[RawToken]) -> Vec<SemanticToken> {
    let mut result = Vec::with_capacity(raw.len());
    let mut prev_line: u32 = 0;
    let mut prev_col: u32 = 0;

    for token in raw {
        let delta_line = token.line - prev_line;
        let delta_start = if delta_line == 0 {
            token.col - prev_col
        } else {
            token.col
        };
        result.push(SemanticToken {
            delta_line,
            delta_start,
            length: token.length,
            token_type: token.token_type,
            token_modifiers_bitset: token.modifiers,
        });
        prev_line = token.line;
        prev_col = token.col;
    }

    result
}
