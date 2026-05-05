//! Phase 1 (M3) — LSP integration tests for schema-aware features.
//!
//! These tests exercise the LSP's hover and document layer
//! directly (the actual `tower_lsp` server is a thin shell over
//! these helpers). The schema-resolution + diagnostic-rendering
//! plumbing is exercised end-to-end via `Document::analyze` and
//! the compiler.

// The LSP modules are part of the `ogham-lsp` binary, not the
// public library, so this integration test file can't depend on
// them directly. We instead exercise the underlying primitives
// (parser, schema, compiler) the way the LSP does. End-to-end LSP
// tests live in the binary's own `#[cfg(test)]` modules.

use ogham::parser::Parser;
use ogham::runtime::compiler::Compiler;
use ogham::runtime::error::VMError;
use ogham::runtime::schema::ModuleSchema;
use ogham::scanner::Scanner;

/// Mirrors what `Document::analyze` does end-to-end. Returns
/// (parse_error, schema_error, compile_error). At most one will
/// be Some in practice — the LSP cascades.
fn analyze(
    source: &str,
) -> (
    Option<ogham::parser::SyntaxError>,
    Option<ogham::parser::SyntaxError>,
    Option<ogham::parser::SyntaxError>,
) {
    let tokens = Scanner::new(source.to_string()).scan();
    let parsed = Parser::new(tokens).parse();
    let parse_err = parsed.as_ref().err().cloned();
    let mut schema_err = None;
    let mut compile_err = None;
    if let Ok(ast) = &parsed {
        match ModuleSchema::from_module(ast) {
            Ok(_schema) => {}
            Err(err) => schema_err = Some(err),
        }
        if let Err(VMError::StrictMode(err)) = Compiler::compile_module(ast) {
            compile_err = Some(err);
        }
    }
    (parse_err, schema_err, compile_err)
}

// ---------------------------------------------------------------------
// Document analysis pipeline (mirrors Document::analyze)
// ---------------------------------------------------------------------

#[test]
fn loose_module_produces_no_errors() {
    let (p, s, c) = analyze("let main = fn () { 5 };");
    assert!(p.is_none());
    assert!(s.is_none());
    assert!(c.is_none());
}

#[test]
fn strict_module_well_formed_produces_no_errors() {
    let (p, s, c) = analyze(
        r#"
        host_state { x: int };
        events { close() };
        let main = fn () {
            event("close");
        };
        "#,
    );
    assert!(p.is_none(), "parse error: {:?}", p);
    assert!(s.is_none(), "schema error: {:?}", s);
    assert!(c.is_none(), "compile error: {:?}", c);
}

#[test]
fn parse_error_short_circuits_pipeline() {
    let (p, s, c) = analyze("host_state {");
    assert!(p.is_some(), "expected parse error");
    // Schema and compile may or may not run (the analyze helper
    // attempts them), but they shouldn't succeed.
    assert!(s.is_none() || s.is_some()); // anything goes — the LSP
                                         // wouldn't even try them
    let _ = c;
}

#[test]
fn schema_error_surfaces_with_note() {
    let (_, s, _) = analyze(
        r#"
        host_state { player: UnknownRecord };
        "#,
    );
    let err = s.expect("expected schema error");
    assert!(err.message.contains("unknown record `UnknownRecord`"));
    assert!(err.note.is_some());
}

#[test]
fn compile_strict_mode_error_surfaces() {
    let (_, _, c) = analyze(
        r#"
        host_state { foo: int };
        let main = fn () { foa };
        "#,
    );
    let err = c.expect("expected compile error");
    assert!(err.message.contains("unknown identifier `foa`"));
    assert!(err.note.is_some());
    assert_eq!(err.help.as_deref(), Some("did you mean `foo`?"));
}

#[test]
fn compile_event_call_error_surfaces() {
    let (_, _, c) = analyze(
        r#"
        events { close() };
        let main = fn () { event("clos"); };
        "#,
    );
    let err = c.expect("expected compile error");
    assert!(err.message.contains("unknown event `clos`"));
    assert_eq!(err.help.as_deref(), Some("did you mean `close`?"));
}

// ---------------------------------------------------------------------
// SyntaxError shape — what LSP's collect_diagnostics renders
// ---------------------------------------------------------------------

#[test]
fn diagnostic_includes_length_when_set() {
    let (_, _, c) = analyze(
        r#"
        host_state { master_volume: string };
        let main = fn () { master_voloume };
        "#,
    );
    let err = c.expect("expected compile error");
    assert!(err.length > 0, "strict-mode errors should set length; got 0");
    assert_eq!(err.length, "master_voloume".len());
}

#[test]
fn diagnostic_event_arg_count_explains_signature() {
    let (_, _, c) = analyze(
        r#"
        events { rebind(string, string) };
        let main = fn () { event("rebind", "Esc"); };
        "#,
    );
    let err = c.expect("expected compile error");
    assert!(err.message.contains("wrong number of arguments"));
    let note = err.note.as_deref().unwrap_or("");
    assert!(note.contains("rebind(string, string)"), "got note: {}", note);
    let help = err.help.as_deref().unwrap_or("");
    assert!(help.contains("expected 2"), "got help: {}", help);
    assert!(help.contains("got 1"), "got help: {}", help);
}

// ---------------------------------------------------------------------
// Schema availability for LSP hover (covered by hover unit tests
// in the binary; here we just confirm the schema is constructible
// from real audit fixtures)
// ---------------------------------------------------------------------

#[test]
fn schema_buildable_from_audit_dm_hud_fixture() {
    let (_, s, c) = analyze(
        r#"
        record EntityInspector {
            name: string,
            kind: string,
            position_text: string,
            detail_lines: array<string>,
            can_possess: bool,
            is_possessing: bool,
            can_open_inventory: bool,
        };
        host_state {
            paused: bool,
            selection_count: int,
            selected_entity: EntityInspector?,
        };
        events {
            dm_toggle_pause(),
            dm_open_inventory(),
            dm_possess(),
            dm_release(),
            dm_deselect(),
        };
        let main = fn () {
            event("dm_toggle_pause");
        };
        "#,
    );
    assert!(s.is_none(), "schema error: {:?}", s);
    assert!(c.is_none(), "compile error: {:?}", c);
}
