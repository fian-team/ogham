use ogham::runtime::value::Value;

/// Top-level `state` declarations should persist across rerenders.
/// Regression test: previously `cleanup_unmounted_state` removed them
/// because their call-stack path is empty and the empty path was never
/// inserted into `active_state_paths`.
#[test]
fn top_level_state_persists_across_rerenders() {
    let source = r#"
state counter = 0;
let main = fn () {
    counter = counter + 1;
    counter
};
"#;
    let mut runtime = ogham::runtime::Runtime::from_source(source, None).expect("from_source");
    let module = runtime.get_module().expect("module").clone();

    let first = runtime.execute_module(&module).expect("first");
    assert_eq!(first, Value::Integer(1), "first execute: 0 + 1");

    let second = runtime.rerender().expect("second");
    assert_eq!(second, Value::Integer(2), "state must persist: 1 + 1");

    let third = runtime.rerender().expect("third");
    assert_eq!(third, Value::Integer(3), "state must persist: 2 + 1");
}

/// A handler-style flow: a nested closure mutates a top-level state var,
/// then the module is rerendered. The mutated value must survive cleanup.
#[test]
fn top_level_state_survives_handler_mutation_then_rerender() {
    let source = r#"
state step = 1;
let bump = fn () { step = step + 1; };
let main = fn () {
    bump();
    step
};
"#;
    let mut runtime = ogham::runtime::Runtime::from_source(source, None).expect("from_source");
    let module = runtime.get_module().expect("module").clone();

    assert_eq!(
        runtime.execute_module(&module).expect("first"),
        Value::Integer(2)
    );
    assert_eq!(runtime.rerender().expect("second"), Value::Integer(3));
    assert_eq!(runtime.rerender().expect("third"), Value::Integer(4));
}
