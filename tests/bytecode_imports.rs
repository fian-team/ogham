//! Integration tests verifying that imported modules are compiled to bytecode
//! and executed in the VM (not tree-walked).

mod common;

use common::execute_file;
use ogham::runtime::value::Value;

#[test]
fn import_all_produces_widget() {
    let value = execute_file("examples/import/importer.ogh");
    assert!(
        matches!(value, Value::Widget(_)),
        "expected a Widget from import-all example, got {:?}",
        value
    );
}

#[test]
fn named_import_produces_widget() {
    let value = execute_file("examples/import/importer_named.ogh");
    assert!(
        matches!(value, Value::Widget(_)),
        "expected a Widget from named-import example, got {:?}",
        value
    );
}

#[test]
fn diamond_imports_produce_widget() {
    let value = execute_file("examples/state_across_closures/main.ogh");
    assert!(
        matches!(value, Value::Widget(_)),
        "expected a Widget from diamond-import example, got {:?}",
        value
    );
}
