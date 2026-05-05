//! Phase 1 (M2) — strict-mode resolution tests.
//!
//! Each test compiles a `.ogh` source string and asserts either:
//! - the compile succeeds (positive case), or
//! - the compile fails with `VMError::StrictMode(SyntaxError)`
//!   carrying expected `message` / `note` / `help` content.
//!
//! Loose-mode tests confirm that modules without a `host_state {}`
//! declaration still compile exactly as they always did.

use ogham::parser::Parser;
use ogham::runtime::compiler::Compiler;
use ogham::runtime::error::VMError;
use ogham::scanner::Scanner;

fn compile(source: &str) -> Result<(), VMError> {
    let tokens = Scanner::new(source.to_string()).scan();
    let module = Parser::new(tokens)
        .parse()
        .expect("parse should succeed");
    Compiler::compile_module(&module).map(|_| ())
}

fn compile_strict_err(source: &str) -> ogham::parser::SyntaxError {
    match compile(source) {
        Err(VMError::StrictMode(err)) => err,
        Err(other) => panic!("expected StrictMode error, got {:?}", other),
        Ok(_) => panic!("expected compile to fail in strict mode"),
    }
}

// ---------------------------------------------------------------------
// Loose-mode regression check (no schema = no strict checks).
// ---------------------------------------------------------------------

#[test]
fn loose_mode_undeclared_identifier_still_compiles() {
    // No host_state {} → loose mode → unknown identifier resolves
    // at runtime (the existing GetState/GetHostState fallback path).
    compile("let main = fn () { unknown_global };").unwrap();
}

#[test]
fn loose_mode_event_with_dynamic_name_compiles() {
    // No strict mode means computed event names are fine.
    compile(
        r#"let main = fn () {
              let n = "click";
              event(n)
           };"#,
    )
    .unwrap();
}

// ---------------------------------------------------------------------
// Strict-mode positive cases (all audit fixtures compile).
// ---------------------------------------------------------------------

#[test]
fn strict_mode_chest_ui_compiles() {
    compile(
        r#"
        events {
            chest_pick_up(),
            chest_cancel(),
        };
        let main = fn () {
            event("chest_pick_up")
        };
        "#,
    )
    .unwrap();
}

#[test]
fn strict_mode_settings_partial_compiles() {
    // Subset of settings_ui's schema; covers a handful of fields,
    // a map type, and several events.
    compile(
        r#"
        host_state {
            master_volume: string,
            invert_y: bool,
            keybinds: map<string, string>,
        };
        events {
            set_master_volume(string),
            toggle_invert_y(),
            close_settings(),
        };
        let main = fn () {
            log master_volume;
            log invert_y;
            event("close_settings");
        };
        "#,
    )
    .unwrap();
}

#[test]
fn strict_mode_dm_hud_with_optional_record_compiles() {
    compile(
        r#"
        record EntityInspector {
            name: string,
            kind: string,
        };
        host_state {
            paused: bool,
            selected_entity: EntityInspector?,
        };
        events {
            dm_toggle_pause(),
            dm_open_inventory(),
        };
        let main = fn () {
            log paused;
            event("dm_toggle_pause");
        };
        "#,
    )
    .unwrap();
}

#[test]
fn strict_mode_record_name_used_in_let_resolves() {
    // Declared records are valid identifiers (even if referencing
    // them outside type position is unusual, it shouldn't error).
    compile(
        r#"
        record Player { name: string };
        host_state { p: Player };
        let main = fn () {
            log p;
        };
        "#,
    )
    .unwrap();
}

// ---------------------------------------------------------------------
// Unknown-identifier diagnostics
// ---------------------------------------------------------------------

#[test]
fn strict_mode_unknown_identifier_errors_with_note() {
    let err = compile_strict_err(
        r#"
        host_state { master_volume: string };
        let main = fn () { master_voloume };
        "#,
    );
    assert!(err.message.contains("unknown identifier `master_voloume`"));
    assert!(err.note.is_some());
    assert!(err.note.as_deref().unwrap().contains("host_state {}"));
}

#[test]
fn strict_mode_unknown_identifier_suggests_close_match() {
    let err = compile_strict_err(
        r#"
        host_state { master_volume: string };
        let main = fn () { master_voloume };
        "#,
    );
    assert_eq!(
        err.help.as_deref(),
        Some("did you mean `master_volume`?")
    );
}

#[test]
fn strict_mode_typo_in_record_field_via_unknown_id() {
    // If the user references `playr` (missing `e`) instead of
    // `player`, the suggestion should fire. The Levenshtein-1
    // check covers single insertions/deletions/substitutions —
    // not transpositions (so `palyer` would NOT trigger the hint;
    // that's a known limitation worth revisiting later).
    let err = compile_strict_err(
        r#"
        record Player { name: string };
        host_state { player: Player };
        let main = fn () { playr };
        "#,
    );
    assert!(err.message.contains("unknown identifier `playr`"));
    assert_eq!(err.help.as_deref(), Some("did you mean `player`?"));
}

#[test]
fn strict_mode_no_close_match_omits_help() {
    let err = compile_strict_err(
        r#"
        host_state { foo: int };
        let main = fn () { zzz_unrelated_thing };
        "#,
    );
    assert!(err.message.contains("unknown identifier"));
    assert!(err.help.is_none(), "got: {:?}", err.help);
}

#[test]
fn strict_mode_locals_are_in_scope_inside_function() {
    // Locals declared via `let` should resolve even in strict mode.
    // (Use `log` to exercise the binary expression rather than a
    // trailing-implicit-return form, which the existing parser
    // doesn't support for binary exprs after an identifier statement.)
    compile(
        r#"
        host_state { foo: int };
        let main = fn () {
            let x = 5;
            log x + foo;
        };
        "#,
    )
    .unwrap();
}

#[test]
fn strict_mode_state_declarations_are_in_scope() {
    compile(
        r#"
        host_state { foo: int };
        let main = fn () {
            state count = 0;
            log count + foo;
        };
        "#,
    )
    .unwrap();
}

#[test]
fn strict_mode_function_params_are_in_scope() {
    compile(
        r#"
        host_state { foo: int };
        let add = fn (a: int, b: int) { log a + b; };
        let main = fn () {
            add(foo, 2);
        };
        "#,
    )
    .unwrap();
}

#[test]
fn strict_mode_builtins_resolve() {
    // `event`, `mutation`, `rgb`, `rgba` should always be valid.
    compile(
        r#"
        host_state { x: int };
        events { close() };
        let main = fn () {
            let c = rgb(255, 0, 0);
            event("close");
            log c;
        };
        "#,
    )
    .unwrap();
}

// ---------------------------------------------------------------------
// Event-call diagnostics
// ---------------------------------------------------------------------

#[test]
fn strict_mode_unknown_event_errors_with_suggestion() {
    let err = compile_strict_err(
        r#"
        events {
            close_settings(),
            set_master_volume(string),
        };
        let main = fn () { event("close_settngs") };
        "#,
    );
    assert!(err.message.contains("unknown event `close_settngs`"));
    assert_eq!(
        err.help.as_deref(),
        Some("did you mean `close_settings`?")
    );
}

#[test]
fn strict_mode_unknown_event_lists_candidates_when_no_close_match() {
    let err = compile_strict_err(
        r#"
        events { close(), foo() };
        let main = fn () { event("zz_unrelated_thing"); };
        "#,
    );
    assert!(err.message.contains("unknown event `zz_unrelated_thing`"));
    let help = err.help.as_deref().unwrap_or("");
    assert!(help.contains("declared events"));
    // Both events should be listed.
    assert!(help.contains("close"));
    assert!(help.contains("foo"));
}

#[test]
fn strict_mode_computed_event_name_errors() {
    let err = compile_strict_err(
        r#"
        events { close() };
        let main = fn () {
            let n = "close";
            event(n);
        };
        "#,
    );
    assert!(err.message.contains("computed event names are not allowed"));
    assert!(err.note.is_some());
}

#[test]
fn strict_mode_event_arg_count_too_few_errors() {
    let err = compile_strict_err(
        r#"
        events { rebind(string, string) };
        let main = fn () { event("rebind", "Esc") };
        "#,
    );
    assert!(err.message.contains("wrong number of arguments"));
    assert!(err.note.as_deref().unwrap().contains("rebind(string, string)"));
    assert!(err.help.as_deref().unwrap().contains("expected 2"));
    assert!(err.help.as_deref().unwrap().contains("got 1"));
}

#[test]
fn strict_mode_event_arg_count_too_many_errors() {
    let err = compile_strict_err(
        r#"
        events { close() };
        let main = fn () { event("close", "extra") };
        "#,
    );
    assert!(err.message.contains("wrong number of arguments"));
    assert!(err.help.as_deref().unwrap().contains("expected 0"));
    assert!(err.help.as_deref().unwrap().contains("got 1"));
}

#[test]
fn strict_mode_event_with_no_args_at_all_errors() {
    let err = compile_strict_err(
        r#"
        events { close() };
        let main = fn () { event() };
        "#,
    );
    assert!(err.message.contains("requires at least an event name"));
}

#[test]
fn strict_mode_record_arg_event_compiles() {
    compile(
        r#"
        record Item { name: string, count: int };
        events { add_item(Item) };
        host_state { x: int };
        let main = fn () {
            log x;
        };
        "#,
    )
    .unwrap();
}

// ---------------------------------------------------------------------
// Closure capture works in strict mode
// ---------------------------------------------------------------------

#[test]
fn strict_mode_closure_captures_host_state() {
    // Closures should be able to capture host_state fields as
    // upvalues. (Important: this exercises the resolve_upvalue path
    // through the strict-mode check.)
    compile(
        r#"
        host_state { volume: string };
        let main = fn () {
            let f = fn () { volume };
            f()
        };
        "#,
    )
    .unwrap();
}

#[test]
fn strict_mode_closure_unknown_identifier_still_errors() {
    let err = compile_strict_err(
        r#"
        host_state { volume: string };
        let main = fn () {
            let f = fn () { volumee };
            f()
        };
        "#,
    );
    assert!(err.message.contains("unknown identifier `volumee`"));
}

#[test]
fn strict_mode_event_call_inside_closure_validated() {
    let err = compile_strict_err(
        r#"
        events { close() };
        host_state { x: int };
        let main = fn () {
            let f = fn () { event("clos") };
            f()
        };
        "#,
    );
    assert!(err.message.contains("unknown event `clos`"));
}
