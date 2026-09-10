//! A container the VM reads is shared, never copied.
//!
//! `Value` is cloned at every read the VM makes — `GetHostState`,
//! `GetLocal`, `GetProperty` — and with `Vec`-backed arrays that clone was
//! a deep copy of the whole container. Read through the language's one
//! iteration idiom, `for (i in 0..rows.length()) { row(rows[i]) }`, every
//! row cost the whole of `rows`: quadratic in a list's length, measured on
//! 2026-09-08 as 194 ms to rerender an 800-row sidebar against 9 ms
//! without the copies. `Value::Array` and `Value::Map` are `Rc`-backed
//! since, and a clone is a refcount bump.
//!
//! Timing tests flake, so this asserts *sharing* rather than speed: the
//! array a document reads out of host state, indexes, and binds to a local
//! must come back out pointing at the allocation the host put in. The day
//! a read path deep-copies again, one of these pointers diverges.

use std::collections::HashMap;
use std::rc::Rc;

use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::runtime::Runtime;

fn three_rows() -> Value {
    Value::array(
        (0..3)
            .map(|i| {
                Value::map(HashMap::from([(
                    "label".to_string(),
                    Value::String(format!("row {i}")),
                )]))
            })
            .collect::<Vec<_>>(),
    )
}

/// Render `source` with `rows` in host state and return the root widget's
/// properties, which is where a document can hand a value back out.
fn properties(source: &str, rows: &Value) -> HashMap<String, Value> {
    let config =
        RuntimeConfig::new().with_host_state(HashMap::from([("rows".to_string(), rows.clone())]));
    let mut runtime = Runtime::from_source(source, Some(config)).expect("from_source");
    let module = runtime.get_module().expect("module").clone();
    match runtime.execute_module(&module).expect("execute") {
        Value::Widget(w) => w.properties,
        other => panic!("expected a widget, got {other:?}"),
    }
}

fn array_of(v: &Value) -> &Rc<Vec<Value>> {
    match v {
        Value::Array(a) => a,
        other => panic!("expected an array, got {other:?}"),
    }
}

fn map_of(v: &Value) -> &Rc<HashMap<String, Value>> {
    match v {
        Value::Map(m) => m,
        other => panic!("expected a map, got {other:?}"),
    }
}

#[test]
fn a_host_array_read_by_a_document_is_the_host_allocation() {
    let rows = three_rows();
    let props = properties(
        r#"
let main = fn () {
    Flex { all: rows }
};
"#,
        &rows,
    );
    assert!(
        Rc::ptr_eq(array_of(&props["all"]), array_of(&rows)),
        "reading host state copied the array"
    );
}

#[test]
fn indexing_a_host_array_shares_the_element() {
    let rows = three_rows();
    let props = properties(
        r#"
let main = fn () {
    Flex { picked: rows[1] }
};
"#,
        &rows,
    );
    let expected = map_of(&array_of(&rows)[1]);
    assert!(
        Rc::ptr_eq(map_of(&props["picked"]), expected),
        "indexing copied the element"
    );
}

#[test]
fn a_local_bound_to_a_host_array_still_shares_it() {
    let rows = three_rows();
    let props = properties(
        r#"
let main = fn () {
    let r = rows;
    Flex { all: r, picked: r[2] }
};
"#,
        &rows,
    );
    assert!(
        Rc::ptr_eq(array_of(&props["all"]), array_of(&rows)),
        "a let binding copied the array"
    );
    assert!(
        Rc::ptr_eq(map_of(&props["picked"]), map_of(&array_of(&rows)[2])),
        "indexing a local copied the element"
    );
}

#[test]
fn a_republished_value_compares_equal_by_pointer_before_by_content() {
    // The host-state diff in `set_host_state` runs `PartialEq`; a host that
    // republishes the value it built last frame is answered by pointer.
    let rows = three_rows();
    let same = rows.clone();
    assert_eq!(rows, same);
    assert!(Rc::ptr_eq(array_of(&rows), array_of(&same)));
    // Equal content in a different allocation is still equal — the pointer
    // path is a fast path, not the rule.
    assert_eq!(rows, three_rows());
}
