//! Phase 1 (M6) — TypedOgham hot-reload tests.
//!
//! Exercises the schema-stable / schema-incompatible reload
//! paths against an actual on-disk file the test rewrites
//! between calls.

use std::fs;
use std::io::Write;

use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::error::RuntimeError;
use ogham::Ogham;
use ogham_derive::{OghamMsg, OghamState};

#[derive(OghamState, PartialEq, Debug, Clone, Default)]
struct CountState {
    count: i32,
    label: String,
}

#[derive(OghamMsg, PartialEq, Debug)]
enum CountMsg {
    Bump,
    Close,
}

const SCHEMA_OGH: &str = r#"
host_state {
    count: int,
    label: string,
};
events {
    bump(),
    close(),
};
let main = fn () {
    log count;
    log label;
    Flex { children: [] }
};
"#;

const SCHEMA_OGH_REORDERED_BODY: &str = r#"
host_state {
    count: int,
    label: string,
};
events {
    bump(),
    close(),
};
let main = fn () {
    // body shape changed; schema unchanged; reload should
    // succeed and preserve typed state.
    log label;
    log count;
    Flex { children: [] }
};
"#;

const SCHEMA_OGH_INCOMPATIBLE: &str = r#"
host_state {
    // schema-breaking change: `count: int` -> `count: string`
    count: string,
    label: string,
};
events {
    bump(),
    close(),
};
let main = fn () {
    log count;
    log label;
    Flex { children: [] }
};
"#;

/// Write a temporary `.ogh` file in target/tmp/ and return its
/// path. The file is reused across rewrites within a single test.
fn write_temp_ogh(name: &str, body: &str) -> String {
    let dir = std::env::temp_dir().join("ogham_typed_hot_reload");
    fs::create_dir_all(&dir).expect("mkdir tmp");
    let path = dir.join(format!("{name}.ogh"));
    let mut f = fs::File::create(&path).expect("create temp .ogh");
    f.write_all(body.as_bytes()).expect("write temp .ogh");
    path.to_string_lossy().into_owned()
}

#[test]
fn schema_compatible_reload_preserves_typed_state() {
    let path = write_temp_ogh("compat", SCHEMA_OGH);
    let mut typed = Ogham::watch_typed::<CountState, CountMsg>(
        path.clone(),
        CountState {
            count: 5,
            label: "initial".into(),
        },
        RuntimeConfig::default(),
    )
    .expect("initial construction");

    // Simulate user updates between renders.
    typed.set_state(CountState {
        count: 99,
        label: "updated".into(),
    });

    // Rewrite the file with a body-only change (schema stable).
    fs::write(&path, SCHEMA_OGH_REORDERED_BODY).expect("rewrite");

    // Reload: succeeds, last_state survives.
    typed.reload().expect("schema-stable reload should succeed");

    // The runtime's host_state now reflects last_state, not the
    // construction-time `initial_state`. We can verify by
    // inspecting via the inner Ogham's runtime.
    let rt = typed.inner().get_runtime().clone();
    let rt = rt.lock().unwrap();
    let count = rt.get_host_state("count").expect("count should be set");
    assert_eq!(count, ogham::runtime::value::Value::Integer(99));
    let label = rt.get_host_state("label").expect("label should be set");
    assert_eq!(
        label,
        ogham::runtime::value::Value::String("updated".into())
    );
}

#[test]
fn schema_incompatible_reload_returns_schema_mismatch() {
    let path = write_temp_ogh("incompat", SCHEMA_OGH);
    let mut typed = Ogham::watch_typed::<CountState, CountMsg>(
        path.clone(),
        CountState::default(),
        RuntimeConfig::default(),
    )
    .expect("initial construction");

    // Rewrite the file with a schema-breaking change
    // (count: int -> count: string).
    fs::write(&path, SCHEMA_OGH_INCOMPATIBLE).expect("rewrite");

    let result = typed.reload();
    let err = result.err().expect("reload should fail on schema mismatch");
    let RuntimeError::SchemaMismatch(diff) = err else {
        panic!("expected SchemaMismatch, got {:?}", err);
    };
    assert!(
        diff.contains("count"),
        "diff should mention count: {}",
        diff
    );
    assert!(diff.contains("type differs"), "diff: {}", diff);

    // The original runtime should be untouched: the count value
    // is still the integer that was set before the reload.
    let rt = typed.inner().get_runtime().clone();
    let rt = rt.lock().unwrap();
    let count = rt.get_host_state("count").expect("count");
    assert!(matches!(count, ogham::runtime::value::Value::Integer(_)));
}

#[test]
fn from_source_typed_reload_is_noop() {
    // No path → reload has nothing to do, returns Ok.
    let mut typed = Ogham::from_source_typed::<CountState, CountMsg>(
        SCHEMA_OGH,
        CountState::default(),
        RuntimeConfig::default(),
    )
    .expect("from_source_typed");
    typed
        .reload()
        .expect("reload should be a no-op for from_source");
}
