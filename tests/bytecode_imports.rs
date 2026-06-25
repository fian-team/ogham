//! Integration tests verifying that imported modules are compiled to bytecode
//! and executed in the VM (not tree-walked).

mod common;

use std::collections::HashMap;
use std::path::PathBuf;

use common::execute_file;
use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::runtime::Runtime;

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

/// Embedded (in-memory) sources resolve `import "..."` with NO filesystem —
/// the path a binary takes when it carries its `.ogh` library via `include_str!`.
/// Exercises transitive imports (entry → lib → dep) entirely from the map.
#[test]
fn embedded_imports_resolve_without_filesystem() {
    let mut sources = HashMap::new();
    // entry → lib → dep, all in-memory. lib re-exports dep's value.
    sources.insert(
        PathBuf::from("./lib.ogh"),
        "import \"./dep.ogh\";\nlet greeting = prefix;".to_string(),
    );
    sources.insert(
        PathBuf::from("./dep.ogh"),
        "let prefix = \"hello\";".to_string(),
    );
    let config = RuntimeConfig::new().with_embedded_sources(sources);

    // Note: no project_root, no files — resolution must come from the map alone.
    let entry = "import \"./lib.ogh\";\nText { text: greeting, style: {} }";
    let mut runtime =
        Runtime::from_source(entry, Some(config)).expect("from_source with embedded imports");
    let module = runtime.get_module().expect("entry module").clone();
    let value = runtime
        .execute_module(&module)
        .expect("execute with embedded (transitive) imports");
    assert!(
        matches!(value, Value::Widget(_)),
        "expected a Widget built from embedded imports, got {:?}",
        value
    );
}
