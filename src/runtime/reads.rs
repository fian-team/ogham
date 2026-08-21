//! What a document reads *through* the names it binds.
//!
//! A `host_state {}` block restates the provider's shapes, so a rename on
//! the provider's side refuses at load: the two copies of the shape stop
//! agreeing and [`Kind::compare`](structure) names the field. A `select`
//! deliberately carries **names only** (`APPLICATION.md` §4.1, §4.6) —
//! shapes are the provider's declaration and a document has no business
//! restating them — and that inversion costs exactly that refusal. With
//! `host_state {}`, a provider that renames `Hud::clock` is refused; with
//! `select manor { hud }`, `hud.clock` reads `Void` and nothing says so.
//!
//! A false expectation is the one thing §4.1 promises never to produce, so
//! the missing half is put back here: the walk below collects every
//! **member-access path off a top-level name** the document makes, and the
//! contract crate resolves each one through the providing scope's
//! reflection at every load and every hot reload.
//!
//! # What this can and cannot see
//!
//! It is syntax, not inference, and it says so honestly. A path is
//! collected only while every step of it is a literal `.field` off a
//! literal identifier. The moment a read goes through anything else, the
//! chain stops and what is left is not reported at all:
//!
//! - `hud.threat[i].filled` collects `hud.threat` and stops. What is on
//!   the far side of an index is not this walk's to know — and §4.2 says
//!   a collection is one field in v1 anyway.
//! - `hud.threat.length()` collects `hud.threat`: a method name is not a
//!   field, so the callee's own chain is what is read.
//! - `hud_pip(hud.threat[i], …)` collects the argument's chain; what the
//!   helper does with its parameter is invisible, because a parameter has
//!   no declared type to resolve against.
//! - a name a `let`, a `state`, a `fn` parameter or a `for` variable binds
//!   is **not** collected, at any depth. That is the false-refusal guard:
//!   a helper whose parameter happens to be called `hud` reads its own
//!   `hud`, and refusing it would be worse than the gap being closed.
//!
//! Nothing here decides whether a read is *wrong* — that needs the
//! provider's reflection, which lives in the structure framework, which
//! this crate has never heard of (§2). This module answers one question in
//! the document's own language: which dotted paths does it read?

use std::collections::BTreeSet;

use crate::parser::{Block, Expression, Function, Literal, Statement};

/// Every dotted read this module makes off a name it did not bind itself,
/// sorted and deduplicated.
///
/// One entry per *longest* chain: `hud.a.b` is collected as `hud.a.b` and
/// not also as `hud.a`, so a missing `a` refuses once rather than twice.
pub fn of(module: &Function) -> Vec<String> {
    let mut found = BTreeSet::new();
    let mut local = Vec::new();
    walk_function(module, &mut local, &mut found);
    found.into_iter().collect()
}

/// A function body, with its parameters bound for the length of it.
fn walk_function(function: &Function, local: &mut Vec<String>, found: &mut BTreeSet<String>) {
    let depth = local.len();
    local.extend(function.arguments.iter().map(|a| a.get()));
    walk_block(&function.body, local, found);
    local.truncate(depth);
}

/// A block, with everything it declares bound for the whole of it.
///
/// Declarations are gathered before the walk rather than as it goes, so a
/// helper referenced above its own `let` is still recognised as local. The
/// cost of getting that wrong is a false refusal, and the cost of being
/// generous is a read nobody checks — an easy trade to make in one
/// direction.
fn walk_block(block: &Block, local: &mut Vec<String>, found: &mut BTreeSet<String>) {
    let depth = local.len();
    for statement in &block.statement_list {
        match statement {
            Statement::Declare(s) => local.push(s.identifier.get()),
            Statement::DeclareState(s) => local.push(s.identifier.get()),
            Statement::ForLoop(s) => local.push(s.variable.get()),
            _ => {}
        }
    }
    for statement in &block.statement_list {
        walk_statement(statement, local, found);
    }
    local.truncate(depth);
}

fn walk_statement(statement: &Statement, local: &mut Vec<String>, found: &mut BTreeSet<String>) {
    match statement {
        Statement::Expression(s) => walk(&s.value, local, found),
        Statement::Declare(s) => walk(&s.value, local, found),
        Statement::DeclareState(s) => walk(&s.value, local, found),
        Statement::Assign(s) => walk(&s.value, local, found),
        Statement::Log(s) => walk(&s.value, local, found),
        Statement::Return(s) => {
            if let Some(value) = &s.value {
                walk(value, local, found);
            }
        }
        Statement::Conditional(s) => {
            for (condition, body) in &s.branches {
                walk(condition, local, found);
                walk_block(body, local, found);
            }
            if let Some(body) = &s.else_block {
                walk_block(body, local, found);
            }
        }
        Statement::ForLoop(s) => {
            walk(&s.range_start, local, found);
            walk(&s.range_end, local, found);
            walk_block(&s.body, local, found);
        }
        // A screen's `view` is document code like any other, and the
        // reads in it are the ones a routed surface actually makes.
        Statement::ScreenDeclaration(s) => walk(&s.view, local, found),
        // Nothing else carries an expression a document can read state
        // through: an import is a path, and the three schema blocks are
        // declarations of shape.
        Statement::Import(_)
        | Statement::RecordDeclaration(_)
        | Statement::HostStateDeclaration(_)
        | Statement::EventsDeclaration(_)
        | Statement::SelectDeclaration(_) => {}
    }
}

fn walk(expression: &Expression, local: &mut Vec<String>, found: &mut BTreeSet<String>) {
    match expression {
        // The one interesting case. A chain that reaches all the way down
        // to a bare identifier is a read; one that does not is walked for
        // whatever it broke on.
        Expression::MemberAccess(access) => match path_of(expression) {
            Some(path) => {
                let root = path.split('.').next().unwrap_or(&path);
                if !local.iter().any(|bound| bound == root) {
                    found.insert(path);
                }
            }
            None => walk(&access.object, local, found),
        },
        // A method name is not a field: `hud.threat.length()` reads
        // `hud.threat`, and `length` belongs to the list's own vocabulary.
        Expression::Call(call) => {
            match call.callee.as_ref() {
                Expression::MemberAccess(access) => walk(&access.object, local, found),
                callee => walk(callee, local, found),
            }
            for argument in &call.arguments {
                walk(argument, local, found);
            }
        }
        Expression::Literal(Literal::Function(function)) => walk_function(function, local, found),
        Expression::Literal(Literal::Array(array)) => {
            for element in &array.elements {
                walk(element, local, found);
            }
        }
        Expression::Literal(Literal::Map(map)) => {
            for (_, value) in &map.properties {
                walk(value, local, found);
            }
        }
        Expression::Literal(_) => {}
        Expression::Widget(widget) => {
            for (_, value) in &widget.properties {
                walk(value, local, found);
            }
        }
        Expression::IndexAccess(access) => {
            walk(&access.object, local, found);
            walk(&access.index, local, found);
        }
        Expression::Unary(unary) => walk(&unary.value, local, found),
        Expression::Binary(binary) => {
            walk(&binary.left, local, found);
            walk(&binary.right, local, found);
        }
        Expression::Grouping(grouping) => walk(&grouping.value, local, found),
        Expression::Range(range) => {
            walk(&range.start, local, found);
            walk(&range.end, local, found);
        }
        Expression::ForLoop(loop_) | Expression::SpreadForLoop(loop_) => {
            walk(&loop_.range_start, local, found);
            walk(&loop_.range_end, local, found);
            let depth = local.len();
            local.push(loop_.variable.get());
            walk_block(&loop_.body, local, found);
            local.truncate(depth);
        }
        Expression::Spread(spread) => walk(&spread.inner, local, found),
        Expression::Match(match_) => {
            walk(&match_.scrutinee, local, found);
            for (pattern, body) in &match_.arms {
                walk(pattern, local, found);
                walk_block(body, local, found);
            }
        }
        // `++x` names an identifier and never a path.
        Expression::PrefixIncrement(_) | Expression::PostfixIncrement(_) => {}
    }
}

/// The dotted path a member-access chain spells, or `None` the moment a
/// step of it is anything but a literal `.field` off a literal name.
fn path_of(expression: &Expression) -> Option<String> {
    match expression {
        Expression::Literal(Literal::Identifier(name)) => Some(name.get()),
        Expression::MemberAccess(access) => Some(format!(
            "{}.{}",
            path_of(&access.object)?,
            access.property.as_str()
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::scanner::Scanner;

    fn reads(source: &str) -> Vec<String> {
        let module = Parser::new(Scanner::new(source.to_string()).scan())
            .parse()
            .expect("parse");
        of(&module)
    }

    #[test]
    fn a_nested_read_is_collected_once_at_its_full_depth() {
        assert_eq!(
            reads("let main = fn () { Text { text: hud.card.title } };"),
            vec!["hud.card.title".to_string()],
            "the longest chain and nothing shorter, so a missing step \
             refuses once"
        );
    }

    #[test]
    fn a_read_through_a_collection_stops_at_the_collection() {
        assert_eq!(
            reads("let main = fn () { Text { text: hud.threat[i].filled } };"),
            vec!["hud.threat".to_string()],
            "a collection is one field in v1, and what is inside one is \
             not this walk's to know"
        );
    }

    #[test]
    fn a_method_name_is_not_a_field() {
        assert_eq!(
            reads("let main = fn () { for (i in 0..hud.threat.length()) { i } };"),
            vec!["hud.threat".to_string()]
        );
    }

    #[test]
    fn a_name_a_parameter_binds_is_not_a_read() {
        assert_eq!(
            reads("let pip = fn (hud: int) { Text { text: hud.filled } };"),
            Vec::<String>::new(),
            "a helper reads its own parameter, and refusing it would be a \
             false refusal — the one outcome worse than the gap"
        );
        assert_eq!(
            reads("let main = fn () { let hud = 1; Text { text: hud.clock } };"),
            Vec::<String>::new()
        );
        assert_eq!(
            reads("let hud = 1;\nlet main = fn () { Text { text: hud.clock } };"),
            Vec::<String>::new(),
            "including a top-level `let`, which is the module's own value"
        );
    }

    #[test]
    fn a_screens_view_is_walked_like_any_other_document_code() {
        assert_eq!(
            reads("screen \"lobby\" { state {} view Text { text: hud.clock } };"),
            vec!["hud.clock".to_string()]
        );
    }

    #[test]
    fn a_read_inside_a_helper_reaches_the_walk() {
        assert_eq!(
            reads(
                "let evening = fn () { Text { text: hud.clock } };\n\
                 let main = fn () { evening() };"
            ),
            vec!["hud.clock".to_string()],
            "regency's thirty-two helper bodies are where every read \
             actually lives"
        );
    }
}
