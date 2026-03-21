use ogham::parser::*;
use tower_lsp::lsp_types::{DocumentSymbol, Position, Range, SymbolKind};

fn span_to_range(span: &Span) -> Range {
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

/// Collect all top-level declarations from the module as LSP document symbols.
pub fn document_symbols(module: &Function) -> Vec<DocumentSymbol> {
    symbols_in_block(&module.body)
}

fn symbols_in_block(block: &Block) -> Vec<DocumentSymbol> {
    let mut out = Vec::new();
    for stmt in &block.statement_list {
        if let Some(sym) = symbol_from_statement(stmt) {
            out.push(sym);
        }
    }
    out
}

#[allow(deprecated)]
fn symbol_from_statement(stmt: &Statement) -> Option<DocumentSymbol> {
    match stmt {
        Statement::Declare(decl) => {
            let (kind, children) = classify_expr(&decl.value);
            Some(DocumentSymbol {
                name: decl.identifier.get(),
                detail: None,
                kind,
                tags: None,
                deprecated: None,
                range: span_to_range(&decl.span),
                selection_range: span_to_range(&decl.identifier.span),
                children,
            })
        }
        Statement::DeclareState(decl) => {
            let (kind, children) = classify_expr(&decl.value);
            Some(DocumentSymbol {
                name: decl.identifier.get(),
                detail: None,
                kind,
                tags: None,
                deprecated: None,
                range: span_to_range(&decl.span),
                selection_range: span_to_range(&decl.identifier.span),
                children,
            })
        }
        Statement::Import(import) => {
            if let Some(names) = &import.names {
                // Return first named import as a MODULE symbol; emit one per name.
                // LSP expects a single symbol here, so we emit the first and rely on
                // the caller iterating — but symbol_from_statement returns one Option.
                // Instead we just emit the path as the symbol name for the whole import.
                let _ = names;
            }
            Some(DocumentSymbol {
                name: import.path.clone(),
                detail: None,
                kind: SymbolKind::MODULE,
                tags: None,
                deprecated: None,
                range: span_to_range(&import.span),
                selection_range: span_to_range(&import.span),
                children: None,
            })
        }
        _ => None,
    }
}

/// Returns (kind, optional children) for a declaration's value expression.
#[allow(deprecated)]
fn classify_expr(expr: &Expression) -> (SymbolKind, Option<Vec<DocumentSymbol>>) {
    if let Expression::Literal(Literal::Function(func)) = expr {
        let children: Vec<DocumentSymbol> = func
            .arguments
            .iter()
            .map(|arg| DocumentSymbol {
                name: arg.get(),
                detail: None,
                kind: SymbolKind::VARIABLE,
                tags: None,
                deprecated: None,
                range: span_to_range(&arg.span),
                selection_range: span_to_range(&arg.span),
                children: None,
            })
            .collect();
        (SymbolKind::FUNCTION, Some(children))
    } else {
        (SymbolKind::VARIABLE, None)
    }
}
