use ogham::parser::*;
use tower_lsp::lsp_types::{Position, Range};

struct Definition {
    name: String,
    decl_span: Span,
}

pub fn span_to_range(span: &Span) -> Range {
    Range {
        start: Position::new(
            span.start_line.saturating_sub(1) as u32,
            span.start_column.saturating_sub(1) as u32,
        ),
        end: Position::new(
            span.end_line.saturating_sub(1) as u32,
            span.end_column.saturating_sub(1) as u32,
        ),
    }
}

/// Return the declaration span for the identifier under (line, col) (1-indexed).
pub fn definition_at(module: &Function, line: usize, col: usize) -> Option<Span> {
    let mut defs: Vec<Definition> = Vec::new();
    def_in_function(module, line, col, &mut defs)
}

fn contains(span: &Span, line: usize, col: usize) -> bool {
    if span.start_line == 0 {
        return false;
    }
    if line < span.start_line || line > span.end_line {
        return false;
    }
    if line == span.start_line && col < span.start_column {
        return false;
    }
    if line == span.end_line && col >= span.end_column {
        return false;
    }
    true
}

fn ident_contains(ident: &Identifier, line: usize, col: usize) -> bool {
    contains(&ident.span, line, col)
}

fn def_in_function(
    func: &Function,
    line: usize,
    col: usize,
    defs: &mut Vec<Definition>,
) -> Option<Span> {
    for arg in &func.arguments {
        if ident_contains(arg, line, col) {
            return Some(arg.span.clone());
        }
        defs.push(Definition {
            name: arg.get(),
            decl_span: arg.span.clone(),
        });
    }
    def_in_block(&func.body, line, col, defs)
}

fn def_in_block(
    block: &Block,
    line: usize,
    col: usize,
    defs: &mut Vec<Definition>,
) -> Option<Span> {
    for stmt in &block.statement_list {
        if let Some(span) = def_in_statement(stmt, line, col, defs) {
            return Some(span);
        }
    }
    None
}

fn def_in_statement(
    stmt: &Statement,
    line: usize,
    col: usize,
    defs: &mut Vec<Definition>,
) -> Option<Span> {
    if !contains(&stmt.span(), line, col) {
        collect_def_from_statement(stmt, defs);
        return None;
    }

    match stmt {
        Statement::Declare(decl) => {
            if ident_contains(&decl.identifier, line, col) {
                return Some(decl.identifier.span.clone());
            }
            defs.push(Definition {
                name: decl.identifier.get(),
                decl_span: decl.identifier.span.clone(),
            });
            def_in_expression(&decl.value, line, col, defs)
        }
        Statement::DeclareState(decl) => {
            if ident_contains(&decl.identifier, line, col) {
                return Some(decl.identifier.span.clone());
            }
            defs.push(Definition {
                name: decl.identifier.get(),
                decl_span: decl.identifier.span.clone(),
            });
            def_in_expression(&decl.value, line, col, defs)
        }
        Statement::Assign(assign) => {
            if ident_contains(&assign.identifier, line, col) {
                return resolve_definition(&assign.identifier.get(), defs);
            }
            def_in_expression(&assign.value, line, col, defs)
        }
        Statement::Expression(expr_stmt) => def_in_expression(&expr_stmt.value, line, col, defs),
        Statement::Return(ret) => {
            if let Some(expr) = &ret.value {
                def_in_expression(expr, line, col, defs)
            } else {
                None
            }
        }
        Statement::Conditional(cond) => {
            for (condition, block) in &cond.branches {
                if let Some(span) = def_in_expression(condition, line, col, defs) {
                    return Some(span);
                }
                if let Some(span) = def_in_block(block, line, col, defs) {
                    return Some(span);
                }
            }
            if let Some(else_block) = &cond.else_block {
                def_in_block(else_block, line, col, defs)
            } else {
                None
            }
        }
        Statement::Log(log) => def_in_expression(&log.value, line, col, defs),
        Statement::ForLoop(for_loop) => {
            if ident_contains(&for_loop.variable, line, col) {
                return Some(for_loop.variable.span.clone());
            }
            defs.push(Definition {
                name: for_loop.variable.get(),
                decl_span: for_loop.variable.span.clone(),
            });
            if let Some(span) = def_in_expression(&for_loop.range_start, line, col, defs) {
                return Some(span);
            }
            if let Some(span) = def_in_expression(&for_loop.range_end, line, col, defs) {
                return Some(span);
            }
            def_in_block(&for_loop.body, line, col, defs)
        }
        Statement::Import(_) => None,
        // Typed-bindings declarations (Phase 1). M3 will add proper
        // schema-aware go-to-definition (record name → declaration);
        // for now, no go-to navigation lands inside these blocks.
        Statement::RecordDeclaration(_)
        | Statement::HostStateDeclaration(_)
        | Statement::EventsDeclaration(_) => None,
        // A screen's `view` is code, not metadata, so unlike the three
        // declarations above it is walked.
        Statement::ScreenDeclaration(decl) => def_in_expression(&decl.view, line, col, defs),
    }
}

fn def_in_expression(
    expr: &Expression,
    line: usize,
    col: usize,
    defs: &mut Vec<Definition>,
) -> Option<Span> {
    if !contains(&expr.span(), line, col) {
        return None;
    }

    match expr {
        Expression::Literal(lit) => def_in_literal(lit, line, col, defs),
        Expression::Unary(u) => def_in_expression(&u.value, line, col, defs),
        Expression::Binary(b) => def_in_expression(&b.left, line, col, defs)
            .or_else(|| def_in_expression(&b.right, line, col, defs)),
        Expression::Grouping(g) => def_in_expression(&g.value, line, col, defs),
        Expression::Widget(w) => {
            for (_, value) in &w.properties {
                if let Some(span) = def_in_expression(value, line, col, defs) {
                    return Some(span);
                }
            }
            None
        }
        Expression::MemberAccess(m) => def_in_expression(&m.object, line, col, defs),
        Expression::Call(c) => {
            if let Some(span) = def_in_expression(&c.callee, line, col, defs) {
                return Some(span);
            }
            for arg in &c.arguments {
                if let Some(span) = def_in_expression(arg, line, col, defs) {
                    return Some(span);
                }
            }
            None
        }
        Expression::IndexAccess(i) => def_in_expression(&i.object, line, col, defs)
            .or_else(|| def_in_expression(&i.index, line, col, defs)),
        Expression::Range(r) => def_in_expression(&r.start, line, col, defs)
            .or_else(|| def_in_expression(&r.end, line, col, defs)),
        Expression::ForLoop(f) | Expression::SpreadForLoop(f) => {
            if ident_contains(&f.variable, line, col) {
                return Some(f.variable.span.clone());
            }
            let saved_len = defs.len();
            defs.push(Definition {
                name: f.variable.get(),
                decl_span: f.variable.span.clone(),
            });
            let result = def_in_expression(&f.range_start, line, col, defs)
                .or_else(|| def_in_expression(&f.range_end, line, col, defs))
                .or_else(|| def_in_block(&f.body, line, col, defs));
            defs.truncate(saved_len);
            result
        }
        Expression::Spread(s) => def_in_expression(&s.inner, line, col, defs),
        Expression::Match(m) => {
            if let Some(span) = def_in_expression(&m.scrutinee, line, col, defs) {
                return Some(span);
            }
            for (pattern, block) in &m.arms {
                if let Some(span) = def_in_expression(pattern, line, col, defs) {
                    return Some(span);
                }
                if let Some(span) = def_in_block(block, line, col, defs) {
                    return Some(span);
                }
            }
            None
        }
        Expression::PrefixIncrement(inc) | Expression::PostfixIncrement(inc) => {
            if ident_contains(&inc.identifier, line, col) {
                return resolve_definition(&inc.identifier.get(), defs);
            }
            None
        }
    }
}

fn def_in_literal(
    lit: &Literal,
    line: usize,
    col: usize,
    defs: &mut Vec<Definition>,
) -> Option<Span> {
    match lit {
        Literal::Identifier(ident) if ident_contains(ident, line, col) => {
            resolve_definition(&ident.get(), defs)
        }
        Literal::Function(func) if contains(&func.span, line, col) => {
            def_in_function(func, line, col, defs)
        }
        Literal::Map(map) if contains(&map.span, line, col) => {
            for (_, value) in &map.properties {
                if let Some(span) = def_in_expression(value, line, col, defs) {
                    return Some(span);
                }
            }
            None
        }
        Literal::Array(array) if contains(&array.span, line, col) => {
            for elem in &array.elements {
                if let Some(span) = def_in_expression(elem, line, col, defs) {
                    return Some(span);
                }
            }
            None
        }
        _ => None,
    }
}

/// Search defs in reverse (innermost scope first). Returns None for unknown identifiers.
fn resolve_definition(name: &str, defs: &[Definition]) -> Option<Span> {
    for def in defs.iter().rev() {
        if def.name == name {
            return Some(def.decl_span.clone());
        }
    }
    None
}

fn collect_def_from_statement(stmt: &Statement, defs: &mut Vec<Definition>) {
    match stmt {
        Statement::Declare(decl) => {
            defs.push(Definition {
                name: decl.identifier.get(),
                decl_span: decl.identifier.span.clone(),
            });
        }
        Statement::DeclareState(decl) => {
            defs.push(Definition {
                name: decl.identifier.get(),
                decl_span: decl.identifier.span.clone(),
            });
        }
        Statement::Import(import) => {
            if let Some(names) = &import.names {
                for n in names {
                    // Imported names have no declaration span in this file; skip.
                    let _ = n;
                }
            }
        }
        _ => {}
    }
}
