use ogham::parser::*;

/// Result of looking up a position in the AST.
pub enum HoverInfo {
    Variable {
        name: String,
        kind: DeclKind,
        value_hint: String,
    },
    Parameter {
        name: String,
    },
    WidgetType {
        name: String,
    },
    Property {
        name: String,
    },
    Literal {
        description: String,
    },
    Keyword {
        name: String,
        description: String,
    },
    ImportPath {
        path: String,
    },
}

#[derive(Clone)]
pub enum DeclKind {
    Let,
    State,
    ForLoop,
    Import,
}

impl HoverInfo {
    pub fn to_markdown(&self) -> String {
        match self {
            HoverInfo::Variable {
                name,
                kind,
                value_hint,
            } => {
                let kw = match kind {
                    DeclKind::Let => "let",
                    DeclKind::State => "state",
                    DeclKind::ForLoop => "for",
                    DeclKind::Import => "import",
                };
                if value_hint.is_empty() {
                    format!("```ogham\n{kw} {name}\n```")
                } else {
                    format!("```ogham\n{kw} {name}: {value_hint}\n```")
                }
            }
            HoverInfo::Parameter { name } => {
                format!("```ogham\nparameter {name}\n```")
            }
            HoverInfo::WidgetType { name } => {
                format!("```ogham\nwidget {name}\n```")
            }
            HoverInfo::Property { name } => {
                format!("```ogham\nproperty {name}\n```")
            }
            HoverInfo::Literal { description } => description.clone(),
            HoverInfo::Keyword { name, description } => {
                format!("**{name}** -- {description}")
            }
            HoverInfo::ImportPath { path } => {
                format!("```ogham\nimport \"{path}\"\n```")
            }
        }
    }
}

/// A declaration seen while walking toward the cursor.
#[derive(Clone)]
struct Declaration {
    name: String,
    kind: DeclKind,
    value_hint: String,
}

/// Compute hover info for a position (1-indexed line and column).
pub fn hover_at(module: &Function, line: usize, col: usize) -> Option<HoverInfo> {
    let mut decls: Vec<Declaration> = Vec::new();
    hover_in_function(module, line, col, &mut decls)
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

fn hover_in_function(
    func: &Function,
    line: usize,
    col: usize,
    decls: &mut Vec<Declaration>,
) -> Option<HoverInfo> {
    for arg in &func.arguments {
        decls.push(Declaration {
            name: arg.get(),
            kind: DeclKind::Let,
            value_hint: String::new(),
        });
        if ident_contains(arg, line, col) {
            return Some(HoverInfo::Parameter { name: arg.get() });
        }
    }
    hover_in_block(&func.body, line, col, decls)
}

fn hover_in_block(
    block: &Block,
    line: usize,
    col: usize,
    decls: &mut Vec<Declaration>,
) -> Option<HoverInfo> {
    for stmt in &block.statement_list {
        if let Some(info) = hover_in_statement(stmt, line, col, decls) {
            return Some(info);
        }
    }
    None
}

fn hover_in_statement(
    stmt: &Statement,
    line: usize,
    col: usize,
    decls: &mut Vec<Declaration>,
) -> Option<HoverInfo> {
    if !contains(&stmt.span(), line, col) {
        collect_decl_from_statement(stmt, decls);
        return None;
    }

    match stmt {
        Statement::Declare(decl) => {
            let hint = value_hint(&decl.value);
            if ident_contains(&decl.identifier, line, col) {
                return Some(HoverInfo::Variable {
                    name: decl.identifier.get(),
                    kind: DeclKind::Let,
                    value_hint: hint,
                });
            }
            decls.push(Declaration {
                name: decl.identifier.get(),
                kind: DeclKind::Let,
                value_hint: hint,
            });
            hover_in_expression(&decl.value, line, col, decls)
        }
        Statement::DeclareState(decl) => {
            let hint = value_hint(&decl.value);
            if ident_contains(&decl.identifier, line, col) {
                return Some(HoverInfo::Variable {
                    name: decl.identifier.get(),
                    kind: DeclKind::State,
                    value_hint: hint,
                });
            }
            decls.push(Declaration {
                name: decl.identifier.get(),
                kind: DeclKind::State,
                value_hint: hint,
            });
            hover_in_expression(&decl.value, line, col, decls)
        }
        Statement::Assign(assign) => {
            if ident_contains(&assign.identifier, line, col) {
                return resolve_ident(&assign.identifier.get(), decls);
            }
            hover_in_expression(&assign.value, line, col, decls)
        }
        Statement::Expression(expr_stmt) => {
            hover_in_expression(&expr_stmt.value, line, col, decls)
        }
        Statement::Return(ret) => {
            if let Some(expr) = &ret.value {
                hover_in_expression(expr, line, col, decls)
            } else {
                Some(HoverInfo::Keyword {
                    name: "return".to_string(),
                    description: "Return from the current function".to_string(),
                })
            }
        }
        Statement::Conditional(cond) => {
            for (condition, block) in &cond.branches {
                if let Some(info) = hover_in_expression(condition, line, col, decls) {
                    return Some(info);
                }
                if let Some(info) = hover_in_block(block, line, col, decls) {
                    return Some(info);
                }
            }
            if let Some(else_block) = &cond.else_block {
                if let Some(info) = hover_in_block(else_block, line, col, decls) {
                    return Some(info);
                }
            }
            None
        }
        Statement::Log(log) => hover_in_expression(&log.value, line, col, decls),
        Statement::ForLoop(for_loop) => {
            if ident_contains(&for_loop.variable, line, col) {
                return Some(HoverInfo::Variable {
                    name: for_loop.variable.get(),
                    kind: DeclKind::ForLoop,
                    value_hint: String::new(),
                });
            }
            decls.push(Declaration {
                name: for_loop.variable.get(),
                kind: DeclKind::ForLoop,
                value_hint: String::new(),
            });
            if let Some(info) = hover_in_expression(&for_loop.range_start, line, col, decls) {
                return Some(info);
            }
            if let Some(info) = hover_in_expression(&for_loop.range_end, line, col, decls) {
                return Some(info);
            }
            hover_in_block(&for_loop.body, line, col, decls)
        }
        Statement::Import(import) => {
            Some(HoverInfo::ImportPath {
                path: import.path.clone(),
            })
        }
    }
}

fn hover_in_expression(
    expr: &Expression,
    line: usize,
    col: usize,
    decls: &mut Vec<Declaration>,
) -> Option<HoverInfo> {
    if !contains(&expr.span(), line, col) {
        return None;
    }

    match expr {
        Expression::Literal(lit) => hover_in_literal(lit, line, col, decls),
        Expression::Unary(u) => hover_in_expression(&u.value, line, col, decls),
        Expression::Binary(b) => {
            if let Some(info) = hover_in_expression(&b.left, line, col, decls) {
                return Some(info);
            }
            hover_in_expression(&b.right, line, col, decls)
        }
        Expression::Grouping(g) => hover_in_expression(&g.value, line, col, decls),
        Expression::Widget(w) => {
            if ident_contains(&w.identifier, line, col) {
                return Some(HoverInfo::WidgetType {
                    name: w.identifier.get(),
                });
            }
            for (key, value) in &w.properties {
                if ident_contains(key, line, col) {
                    return Some(HoverInfo::Property { name: key.get() });
                }
                if let Some(info) = hover_in_expression(value, line, col, decls) {
                    return Some(info);
                }
            }
            None
        }
        Expression::MemberAccess(m) => {
            if ident_contains(&m.property, line, col) {
                return Some(HoverInfo::Property {
                    name: m.property.get(),
                });
            }
            hover_in_expression(&m.object, line, col, decls)
        }
        Expression::Call(c) => {
            if let Some(info) = hover_in_expression(&c.callee, line, col, decls) {
                return Some(info);
            }
            for arg in &c.arguments {
                if let Some(info) = hover_in_expression(arg, line, col, decls) {
                    return Some(info);
                }
            }
            None
        }
        Expression::IndexAccess(i) => {
            if let Some(info) = hover_in_expression(&i.object, line, col, decls) {
                return Some(info);
            }
            hover_in_expression(&i.index, line, col, decls)
        }
        Expression::Range(r) => {
            if let Some(info) = hover_in_expression(&r.start, line, col, decls) {
                return Some(info);
            }
            hover_in_expression(&r.end, line, col, decls)
        }
        Expression::ForLoop(f) | Expression::SpreadForLoop(f) => {
            if ident_contains(&f.variable, line, col) {
                return Some(HoverInfo::Variable {
                    name: f.variable.get(),
                    kind: DeclKind::ForLoop,
                    value_hint: String::new(),
                });
            }
            let saved_len = decls.len();
            decls.push(Declaration {
                name: f.variable.get(),
                kind: DeclKind::ForLoop,
                value_hint: String::new(),
            });
            let result = hover_in_expression(&f.range_start, line, col, decls)
                .or_else(|| hover_in_expression(&f.range_end, line, col, decls))
                .or_else(|| hover_in_block(&f.body, line, col, decls));
            decls.truncate(saved_len);
            result
        }
        Expression::Spread(s) => hover_in_expression(&s.inner, line, col, decls),
        Expression::Match(m) => {
            if let Some(info) = hover_in_expression(&m.scrutinee, line, col, decls) {
                return Some(info);
            }
            for (pattern, block) in &m.arms {
                if let Some(info) = hover_in_expression(pattern, line, col, decls) {
                    return Some(info);
                }
                if let Some(info) = hover_in_block(block, line, col, decls) {
                    return Some(info);
                }
            }
            None
        }
        Expression::PrefixIncrement(inc) | Expression::PostfixIncrement(inc) => {
            if ident_contains(&inc.identifier, line, col) {
                return resolve_ident(&inc.identifier.get(), decls);
            }
            None
        }
    }
}

fn hover_in_literal(
    lit: &Literal,
    line: usize,
    col: usize,
    decls: &mut Vec<Declaration>,
) -> Option<HoverInfo> {
    match lit {
        Literal::Integer(v, span) if contains(span, line, col) => {
            Some(HoverInfo::Literal {
                description: format!("`{v}` -- integer literal"),
            })
        }
        Literal::Float(v, span) if contains(span, line, col) => {
            Some(HoverInfo::Literal {
                description: format!("`{v}` -- float literal"),
            })
        }
        Literal::Boolean(v, span) if contains(span, line, col) => {
            Some(HoverInfo::Literal {
                description: format!("`{v}` -- boolean literal"),
            })
        }
        Literal::String(v, span) if contains(span, line, col) => {
            Some(HoverInfo::Literal {
                description: format!("`\"{v}\"` -- string literal"),
            })
        }
        Literal::Identifier(ident) if ident_contains(ident, line, col) => {
            resolve_ident(&ident.get(), decls)
        }
        Literal::Function(func) if contains(&func.span, line, col) => {
            hover_in_function(func, line, col, decls)
        }
        Literal::Map(map) if contains(&map.span, line, col) => {
            for (key, value) in &map.properties {
                if ident_contains(key, line, col) {
                    return Some(HoverInfo::Property { name: key.get() });
                }
                if let Some(info) = hover_in_expression(value, line, col, decls) {
                    return Some(info);
                }
            }
            None
        }
        Literal::Array(array) if contains(&array.span, line, col) => {
            for elem in &array.elements {
                if let Some(info) = hover_in_expression(elem, line, col, decls) {
                    return Some(info);
                }
            }
            None
        }
        _ => None,
    }
}

fn resolve_ident(name: &str, decls: &[Declaration]) -> Option<HoverInfo> {
    for decl in decls.iter().rev() {
        if decl.name == name {
            return Some(HoverInfo::Variable {
                name: name.to_string(),
                kind: decl.kind.clone(),
                value_hint: decl.value_hint.clone(),
            });
        }
    }
    Some(HoverInfo::Variable {
        name: name.to_string(),
        kind: DeclKind::Let,
        value_hint: String::new(),
    })
}

fn collect_decl_from_statement(stmt: &Statement, decls: &mut Vec<Declaration>) {
    match stmt {
        Statement::Declare(decl) => {
            decls.push(Declaration {
                name: decl.identifier.get(),
                kind: DeclKind::Let,
                value_hint: value_hint(&decl.value),
            });
        }
        Statement::DeclareState(decl) => {
            decls.push(Declaration {
                name: decl.identifier.get(),
                kind: DeclKind::State,
                value_hint: value_hint(&decl.value),
            });
        }
        Statement::Import(import) => {
            if let Some(names) = &import.names {
                for n in names {
                    decls.push(Declaration {
                        name: n.clone(),
                        kind: DeclKind::Import,
                        value_hint: String::new(),
                    });
                }
            }
        }
        _ => {}
    }
}

fn value_hint(expr: &Expression) -> String {
    match expr {
        Expression::Literal(Literal::Integer(v, _)) => format!("integer = {v}"),
        Expression::Literal(Literal::Float(v, _)) => format!("float = {v}"),
        Expression::Literal(Literal::Boolean(v, _)) => format!("boolean = {v}"),
        Expression::Literal(Literal::String(v, _)) => format!("string = \"{v}\""),
        Expression::Literal(Literal::Function(f)) => {
            format!("function({} params)", f.arguments.len())
        }
        Expression::Literal(Literal::Map(_)) => "map".to_string(),
        Expression::Literal(Literal::Array(_)) => "array".to_string(),
        Expression::Widget(w) => format!("widget {}", w.identifier.get()),
        _ => String::new(),
    }
}
