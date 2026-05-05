//! Phase 2 M2 — `effect` + `cleanup` integration tests.
//!
//! Covers parser acceptance/rejection, runtime fire semantics
//! (first-render, dep-change, dep-unchanged, empty-deps,
//! cleanup-before-refire, cleanup-on-unmount), and LSP
//! warnings.

use std::sync::{Arc, Mutex};

use ogham::parser::{Parser, Statement};
use ogham::runtime::value::Value;
use ogham::runtime::Runtime;
use ogham::scanner::Scanner;

fn parse(src: &str) -> Result<ogham::parser::Function, ogham::parser::SyntaxError> {
    let mut s = Scanner::new(src.to_string());
    let mut p = Parser::new(s.scan());
    p.parse()
}

fn run_with_log(src: &str) -> (Runtime, Arc<Mutex<Vec<String>>>) {
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let log_for = Arc::clone(&log);
    let mut runtime = Runtime::from_source(src, None).expect("parse");
    runtime.register_event_handler("test_log", move |args| {
        if let Some(Value::String(s)) = args.first() {
            log_for.lock().unwrap().push(s.clone());
        }
        Ok(Value::Void)
    });
    let module = runtime.get_module().expect("module").clone();
    runtime.execute_module(&module).expect("execute");
    (runtime, log)
}

// -----------------------------------------------------------------
// Parser
// -----------------------------------------------------------------

#[test]
fn parses_effect_with_single_dep() {
    let src = "let main = fn () { effect (1) { log \"x\"; }; Flex { children: [] } };";
    parse(src).expect("parse");
}

#[test]
fn parses_effect_with_multiple_deps() {
    let src = "let main = fn () { effect (1, 2, 3) { log \"x\"; }; Flex { children: [] } };";
    parse(src).expect("parse");
}

#[test]
fn parses_effect_with_empty_deps() {
    let src = "let main = fn () { effect () { log \"once\"; }; Flex { children: [] } };";
    parse(src).expect("parse");
}

#[test]
fn parses_cleanup_inside_effect() {
    let src = r#"
let main = fn () {
  effect (1) {
    log "fire";
    cleanup { log "teardown"; };
  };
  Flex { children: [] }
};
"#;
    parse(src).expect("parse");
}

#[test]
fn cleanup_outside_effect_compile_error() {
    // Parser accepts `cleanup` as a statement; the *compiler*
    // rejects it outside an effect body (it's a strict-mode
    // error from Compiler::compile_statement).
    let src = r#"
let main = fn () {
  cleanup { log "lone"; };
  Flex { children: [] }
};
"#;
    let module = parse(src).expect("parse");
    let result = ogham::runtime::compiler::Compiler::compile_module(&module);
    assert!(result.is_err(), "compiler should reject lone cleanup");
    let err = result.unwrap_err();
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("cleanup") || msg.contains("effect"),
        "error should mention cleanup/effect; got {}",
        msg
    );
}

// -----------------------------------------------------------------
// Runtime — fire semantics
// -----------------------------------------------------------------

#[test]
fn effect_fires_on_first_render() {
    let src = r#"
let main = fn () {
  effect (1) { event("test_log", "fired"); };
  Flex { children: [] }
};
"#;
    let (_runtime, log) = run_with_log(src);
    let entries = log.lock().unwrap().clone();
    assert_eq!(entries, vec!["fired".to_string()]);
}

#[test]
fn effect_does_not_fire_when_deps_unchanged() {
    // Same dep value across renders — effect fires once on
    // first render, never again.
    let src = r#"
let main = fn () {
  effect (1) { event("test_log", "fired"); };
  Flex { children: [] }
};
"#;
    let (mut runtime, log) = run_with_log(src);
    runtime.rerender().expect("second render");
    runtime.rerender().expect("third render");
    let entries = log.lock().unwrap().clone();
    assert_eq!(entries.len(), 1, "effect should fire only once");
}

#[test]
fn effect_re_fires_when_dep_changes() {
    // Dep is a host_state value — flip it between renders.
    let src = r#"
let main = fn () {
  effect (counter) { event("test_log", "fired"); };
  Flex { children: [] }
};
"#;
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let log_for = Arc::clone(&log);
    let mut runtime = Runtime::from_source(src, None).expect("parse");
    runtime.register_event_handler("test_log", move |args| {
        if let Some(Value::String(s)) = args.first() {
            log_for.lock().unwrap().push(s.clone());
        }
        Ok(Value::Void)
    });
    runtime.inject_host_state("counter".to_string(), Value::Integer(0));
    let module = runtime.get_module().expect("module").clone();
    runtime.execute_module(&module).expect("first render");

    // Same value → no fire.
    runtime.rerender().expect("second render");
    assert_eq!(
        log.lock().unwrap().len(),
        1,
        "first render fires; second render same dep doesn't"
    );

    // Change value → fires again.
    runtime.inject_host_state("counter".to_string(), Value::Integer(1));
    runtime.rerender().expect("third render");
    assert_eq!(
        log.lock().unwrap().len(),
        2,
        "dep change should refire"
    );
}

#[test]
fn effect_with_empty_deps_fires_only_once() {
    // Empty deps — fires once on first render, never refires.
    let src = r#"
let main = fn () {
  effect () { event("test_log", "fired"); };
  Flex { children: [] }
};
"#;
    let (mut runtime, log) = run_with_log(src);
    runtime.rerender().expect("second render");
    runtime.rerender().expect("third render");
    let entries = log.lock().unwrap().clone();
    assert_eq!(entries.len(), 1, "empty-deps effect fires once");
}

#[test]
fn cleanup_runs_before_effect_re_fires() {
    let src = r#"
let main = fn () {
  effect (counter) {
    event("test_log", "fire");
    cleanup { event("test_log", "clean"); };
  };
  Flex { children: [] }
};
"#;
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let log_for = Arc::clone(&log);
    let mut runtime = Runtime::from_source(src, None).expect("parse");
    runtime.register_event_handler("test_log", move |args| {
        if let Some(Value::String(s)) = args.first() {
            log_for.lock().unwrap().push(s.clone());
        }
        Ok(Value::Void)
    });
    runtime.inject_host_state("counter".to_string(), Value::Integer(0));
    let module = runtime.get_module().expect("module").clone();
    runtime.execute_module(&module).expect("first render");

    // First render: fire, cleanup registered.
    let entries = log.lock().unwrap().clone();
    assert_eq!(entries, vec!["fire".to_string()]);

    // Change dep → cleanup runs first, then fire.
    runtime.inject_host_state("counter".to_string(), Value::Integer(1));
    runtime.rerender().expect("second render");
    let entries = log.lock().unwrap().clone();
    assert_eq!(
        entries,
        vec![
            "fire".to_string(),
            "clean".to_string(),
            "fire".to_string()
        ],
        "cleanup runs before re-fire"
    );
}

#[test]
fn cleanup_runs_when_path_unmounts() {
    let src = r#"
let panel = fn () {
  effect () {
    event("test_log", "fire");
    cleanup { event("test_log", "clean"); };
  };
  Flex { children: [] }
};
let main = fn () {
  if (show) {
    return panel();
  } else {
    return Flex { children: [] };
  }
};
"#;
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let log_for = Arc::clone(&log);
    let mut runtime = Runtime::from_source(src, None).expect("parse");
    runtime.register_event_handler("test_log", move |args| {
        if let Some(Value::String(s)) = args.first() {
            log_for.lock().unwrap().push(s.clone());
        }
        Ok(Value::Void)
    });
    runtime.inject_host_state("show".to_string(), Value::Boolean(true));
    let module = runtime.get_module().expect("module").clone();
    runtime.execute_module(&module).expect("first render");

    // First render: fire.
    let entries = log.lock().unwrap().clone();
    assert_eq!(entries, vec!["fire".to_string()]);

    // Hide panel → path disappears → cleanup runs.
    runtime.inject_host_state("show".to_string(), Value::Boolean(false));
    runtime.rerender().expect("second render");
    let entries = log.lock().unwrap().clone();
    assert!(
        entries.contains(&"clean".to_string()),
        "cleanup should fire on unmount; got {:?}",
        entries
    );
}

// -----------------------------------------------------------------
// Scanner
// -----------------------------------------------------------------

#[test]
fn scanner_recognizes_effect_and_cleanup() {
    let mut s = Scanner::new("effect cleanup".to_string());
    let tokens = s.scan();
    assert!(matches!(
        tokens[0].token_type,
        ogham::scanner::TokenType::Effect
    ));
    assert!(matches!(
        tokens[1].token_type,
        ogham::scanner::TokenType::Cleanup
    ));
}

// -----------------------------------------------------------------
// Hover smoke
// -----------------------------------------------------------------

// -----------------------------------------------------------------
// Dep type check (audit fix 3)
// -----------------------------------------------------------------

#[test]
fn effect_dep_rejects_direct_fn_literal() {
    let src = r#"
let main = fn () {
  effect (fn () { 1 }) { log "x"; };
  Flex { children: [] }
};
"#;
    let module = parse(src).expect("parse");
    let result = ogham::runtime::compiler::Compiler::compile_module(&module);
    assert!(result.is_err(), "fn literal as dep must be rejected");
    let err = format!("{:?}", result.unwrap_err());
    assert!(
        err.contains("primitive or record value"),
        "diagnostic message mismatch: {}",
        err
    );
}

#[test]
fn effect_dep_rejects_known_fn_typed_let() {
    let src = r#"
let helper = fn () { 1 };
let main = fn () {
  effect (helper) { log "x"; };
  Flex { children: [] }
};
"#;
    let module = parse(src).expect("parse");
    let result = ogham::runtime::compiler::Compiler::compile_module(&module);
    assert!(
        result.is_err(),
        "identifier resolving to fn-typed let must be rejected"
    );
    let err = format!("{:?}", result.unwrap_err());
    assert!(
        err.contains("'helper'") && err.contains("function"),
        "diagnostic should name the binding; got: {}",
        err
    );
}

#[test]
fn effect_dep_accepts_primitive_values() {
    let src = r#"
let main = fn () {
  state n = 0;
  effect (n, true, "label") { log "x"; };
  Flex { children: [] }
};
"#;
    let module = parse(src).expect("parse");
    ogham::runtime::compiler::Compiler::compile_module(&module)
        .expect("primitive deps must compile");
}

// -----------------------------------------------------------------
// Audit follow-up tests (fixes 4-5)
// -----------------------------------------------------------------

#[test]
fn multiple_cleanup_blocks_only_last_wins() {
    // RegisterEffectCleanup overwrites the slot's
    // pending_cleanup. So if an effect body calls it twice
    // (via two cleanup blocks), only the second one fires.
    let src = r#"
let main = fn () {
  state n = 0;
  effect (n) {
    cleanup { event("test_log", "first"); };
    cleanup { event("test_log", "second"); };
    n = n + 1;
  };
  Flex { children: [] }
};
"#;
    // We can't easily change `n` from outside since it's a
    // state cell. Instead: use a host_state-driven dep so we
    // can flip it across renders.
    let src = r#"
let main = fn () {
  effect (counter) {
    cleanup { event("test_log", "first"); };
    cleanup { event("test_log", "second"); };
  };
  Flex { children: [] }
};
"#;
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let log_for = Arc::clone(&log);
    let mut runtime = Runtime::from_source(src, None).expect("parse");
    runtime.register_event_handler("test_log", move |args| {
        if let Some(Value::String(s)) = args.first() {
            log_for.lock().unwrap().push(s.clone());
        }
        Ok(Value::Void)
    });
    runtime.inject_host_state("counter".to_string(), Value::Integer(0));
    let module = runtime.get_module().expect("module").clone();
    runtime.execute_module(&module).expect("first render");

    // Trigger re-fire → cleanup should run.
    runtime.inject_host_state("counter".to_string(), Value::Integer(1));
    runtime.rerender().expect("second render");

    let entries = log.lock().unwrap().clone();
    assert_eq!(
        entries,
        vec!["second".to_string()],
        "only the LAST cleanup should fire (later RegisterEffectCleanup overwrites)"
    );
}

#[test]
fn multiple_effects_in_one_fn_fire_in_source_order() {
    let src = r#"
let main = fn () {
  effect (counter) { event("test_log", "a"); };
  effect (counter) { event("test_log", "b"); };
  effect (counter) { event("test_log", "c"); };
  Flex { children: [] }
};
"#;
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let log_for = Arc::clone(&log);
    let mut runtime = Runtime::from_source(src, None).expect("parse");
    runtime.register_event_handler("test_log", move |args| {
        if let Some(Value::String(s)) = args.first() {
            log_for.lock().unwrap().push(s.clone());
        }
        Ok(Value::Void)
    });
    runtime.inject_host_state("counter".to_string(), Value::Integer(0));
    let module = runtime.get_module().expect("module").clone();
    runtime.execute_module(&module).expect("first render");

    let entries = log.lock().unwrap().clone();
    assert_eq!(
        entries,
        vec!["a".to_string(), "b".to_string(), "c".to_string()],
        "effects should fire in source order on first render"
    );

    // Re-fire (dep change) — order should be preserved.
    log.lock().unwrap().clear();
    runtime.inject_host_state("counter".to_string(), Value::Integer(1));
    runtime.rerender().expect("second render");
    let entries = log.lock().unwrap().clone();
    assert_eq!(
        entries,
        vec!["a".to_string(), "b".to_string(), "c".to_string()],
        "effects should fire in source order on dep-change re-fire"
    );
}

#[test]
fn effect_statement_is_a_distinct_ast_variant() {
    let src = "let main = fn () { effect (1) {}; Flex { children: [] } };";
    let module = parse(src).expect("parse");
    // Walk into main's body to find the Effect statement.
    let mut found_effect = false;
    if let Statement::Declare(d) = &module.body.statement_list[0] {
        if let ogham::parser::Expression::Literal(ogham::parser::Literal::Function(f)) =
            &d.value
        {
            for s in &f.body.statement_list {
                if matches!(s, Statement::Effect(_)) {
                    found_effect = true;
                }
            }
        }
    }
    assert!(found_effect, "should find Statement::Effect in main body");
}
