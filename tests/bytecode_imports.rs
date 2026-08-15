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

/// A strict module (`host_state {}` declared) referencing a helper an
/// import provides. The runtime pre-scans imports for the names they
/// carry and hands them to strict identifier resolution — the promise
/// the strict-mode diagnostic makes ("… imports, records, and
/// built-ins"). Without the pre-scan this fails to compile with
/// "unknown identifier", which is exactly the regression this pins.
#[test]
fn a_strict_module_can_call_an_imported_helper() {
    let mut sources = HashMap::new();
    sources.insert(
        PathBuf::from("./panel.ogh"),
        "let panel_title = fn (t: string) { Text { text: t, style: {} } };".to_string(),
    );
    let config = RuntimeConfig::new().with_embedded_sources(sources);

    let entry = r#"
import "./panel.ogh";
host_state { title: string };
let main = fn () { panel_title(title) };
main()
"#;
    let mut runtime =
        Runtime::from_source(entry, Some(config)).expect("from_source with embedded imports");
    runtime.inject_host_state("title".to_string(), Value::String("tide".to_string()));
    let module = runtime.get_module().expect("entry module").clone();
    let value = runtime
        .execute_module(&module)
        .expect("a strict module may call what it imports");
    assert!(
        matches!(value, Value::Widget(_)),
        "expected a Widget, got {:?}",
        value
    );
}

/// The strict guard itself must survive the import pre-scan: a name the
/// import does NOT provide is still an unknown identifier.
#[test]
fn a_strict_module_still_rejects_unknown_identifiers_beside_imports() {
    let mut sources = HashMap::new();
    sources.insert(
        PathBuf::from("./panel.ogh"),
        "let panel_title = fn (t: string) { Text { text: t, style: {} } };".to_string(),
    );
    let config = RuntimeConfig::new().with_embedded_sources(sources);

    let entry = r#"
import "./panel.ogh";
host_state { title: string };
let main = fn () { panel_titel(title) };
main()
"#;
    let mut runtime =
        Runtime::from_source(entry, Some(config)).expect("from_source parses");
    let module = runtime.get_module().expect("entry module").clone();
    let err = runtime
        .execute_module(&module)
        .expect_err("a typo is still a typo");
    assert!(
        format!("{err:?}").contains("panel_titel"),
        "expected the unknown identifier named, got {err:?}"
    );
}
