//! P0-M1 — `#[ogham(binding_module = "...")]` attribute smoke tests.
//!
//! These tests live alongside `derive_smoke.rs` because proc-macro
//! integration tests need to depend on the consumer-facing `ogham`
//! crate (the derive's emitted code references
//! `::ogham::runtime::schema::*` paths).
//!
//! P0-M1 only validates that the attribute is *accepted* — no
//! manifest is written yet. P0-M3 wires the actual emit and
//! extends these tests to assert on disk output.

use std::collections::HashMap;
use std::path::PathBuf;

use ogham::diagnostics::{EventsManifest, Manifest, ManifestEvent, ManifestField, StateManifest};
use ogham::runtime::schema::{OghamMsg, OghamRecord, PrimType, TypeRef};
use ogham_derive::{OghamMsg, OghamRecord, OghamState};

/// Resolve the path the derive will have written a manifest to.
/// Mirrors the `manifest_path` formula in `ogham-derive::manifest_emit`.
fn manifest_path(kind: &str, binding_module: &str, type_name: &str) -> PathBuf {
    let target = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target")
        });
    let sanitized: String = binding_module
        .chars()
        .map(|c| match c {
            '/' | '\\' | '.' => '_',
            other => other,
        })
        .collect();
    let crate_name: String = env!("CARGO_PKG_NAME")
        .chars()
        .map(|c| match c {
            '/' | '\\' | '.' => '_',
            other => other,
        })
        .collect();
    target
        .join("ogham")
        .join(format!("{kind}-{crate_name}-{sanitized}-{type_name}.json"))
}

#[derive(OghamState, Clone, PartialEq, Debug)]
#[ogham(binding_module = "data/ui/chest.ogh")]
struct ChestUiState {
    selected: i32,
    open: bool,
}

#[derive(OghamMsg, Clone, PartialEq, Debug)]
#[ogham(binding_module = "data/ui/chest.ogh")]
enum ChestUiMsg {
    OpenChest,
    TakeItem(i32),
    CloseChest,
}

#[test]
fn state_with_binding_module_compiles_and_schema_unchanged() {
    // The attribute is currently a no-op; the derived schema must
    // be identical to what we'd get without it. This guards against
    // the parser silently swallowing other behaviour as we extend it.
    assert_eq!(ChestUiState::OGHAM_RECORD_NAME, "ChestUiState");
    let schema = ChestUiState::ogham_record_schema();
    assert_eq!(schema.fields.len(), 2);
    assert_eq!(
        schema.fields["selected"].ty,
        TypeRef::Primitive(PrimType::Int),
    );
    assert_eq!(
        schema.fields["open"].ty,
        TypeRef::Primitive(PrimType::Bool),
    );
}

#[test]
fn msg_with_binding_module_compiles_and_events_unchanged() {
    let events = ChestUiMsg::ogham_events();
    assert_eq!(events.len(), 3);
    assert!(events.contains_key("open_chest"));
    assert!(events.contains_key("take_item"));
    assert!(events.contains_key("close_chest"));
    assert_eq!(
        events["take_item"].args,
        vec![TypeRef::Primitive(PrimType::Int)],
    );
}

// ---------------------------------------------------------------------
// P0-M3 — manifest emit fixtures + on-disk round-trip tests.
// ---------------------------------------------------------------------

// Fixtures are used by the macros at compile time to emit manifests;
// the tests read those manifests off disk rather than instantiating
// the types, so the compiler sees them as dead. Allow.
#[allow(dead_code)]
#[derive(OghamRecord, Clone, PartialEq, Debug)]
struct Item {
    name: String,
    quantity: i32,
}

/// Fixture exercising the container-type mappings:
/// `Vec<T>` → `array<T>`, `Option<T>` → `T?`, `HashMap<String, V>` →
/// `map<string, V>`. Distinct `binding_module` so it produces its
/// own manifest filename.
#[allow(dead_code)]
#[derive(OghamState, Clone, PartialEq, Debug)]
#[ogham(binding_module = "data/ui/inventory.ogh")]
struct InventoryUiState {
    items: Vec<Item>,
    selected_index: Option<i32>,
    quantities_by_name: HashMap<String, i32>,
}

#[allow(dead_code)]
#[derive(OghamMsg, Clone, PartialEq, Debug)]
#[ogham(binding_module = "data/ui/inventory.ogh")]
enum InventoryUiMsg {
    Open,
    Take(i32, String),
    Close,
}

#[test]
fn state_manifest_lands_on_disk_with_expected_shape() {
    let path = manifest_path("state", "data/ui/chest.ogh", "ChestUiState");
    let manifest = Manifest::read(&path)
        .unwrap_or_else(|e| panic!("expected manifest at {}: {e}", path.display()));
    let Manifest::State(state) = manifest else {
        panic!("expected state-kind manifest, got events");
    };
    assert_eq!(state.binding, "ogham::ChestUiState");
    assert_eq!(state.ogh_module, "data/ui/chest.ogh");
    assert_eq!(state.host_state.fields.len(), 2);
    assert_eq!(
        state.host_state.fields.get("selected"),
        Some(&ManifestField { ty: "int".into() }),
    );
    assert_eq!(
        state.host_state.fields.get("open"),
        Some(&ManifestField { ty: "bool".into() }),
    );
    // P0-M3 ships with empty source location — file unavailable on
    // stable proc-macro. See manifest_emit.rs module docs.
    assert_eq!(state.rust_source.file, "");
    assert_eq!(state.rust_source.line, 0);
}

#[test]
fn events_manifest_lands_on_disk_with_expected_shape() {
    let path = manifest_path("events", "data/ui/chest.ogh", "ChestUiMsg");
    let manifest = Manifest::read(&path)
        .unwrap_or_else(|e| panic!("expected manifest at {}: {e}", path.display()));
    let Manifest::Events(events) = manifest else {
        panic!("expected events-kind manifest, got state");
    };
    assert_eq!(events.binding, "ogham::ChestUiMsg");
    assert_eq!(events.ogh_module, "data/ui/chest.ogh");
    assert_eq!(events.events.len(), 3);
    assert_eq!(
        events.events.get("open_chest"),
        Some(&ManifestEvent { args: vec![] }),
    );
    assert_eq!(
        events.events.get("take_item"),
        Some(&ManifestEvent {
            args: vec!["int".into()],
        }),
    );
    assert_eq!(
        events.events.get("close_chest"),
        Some(&ManifestEvent { args: vec![] }),
    );
}

#[test]
fn manifest_captures_container_types_via_canonical_string() {
    let path = manifest_path("state", "data/ui/inventory.ogh", "InventoryUiState");
    let manifest = Manifest::read(&path)
        .unwrap_or_else(|e| panic!("expected manifest at {}: {e}", path.display()));
    let Manifest::State(state) = manifest else {
        panic!("expected state-kind manifest");
    };
    assert_eq!(state.host_state.fields.len(), 3);
    assert_eq!(
        state.host_state.fields.get("items").map(|f| f.ty.as_str()),
        Some("array<Item>"),
    );
    assert_eq!(
        state
            .host_state
            .fields
            .get("selected_index")
            .map(|f| f.ty.as_str()),
        Some("int?"),
    );
    assert_eq!(
        state
            .host_state
            .fields
            .get("quantities_by_name")
            .map(|f| f.ty.as_str()),
        Some("map<string, int>"),
    );
}

#[test]
fn events_manifest_captures_multi_arg_signatures() {
    let path = manifest_path("events", "data/ui/inventory.ogh", "InventoryUiMsg");
    let manifest = Manifest::read(&path)
        .unwrap_or_else(|e| panic!("expected manifest at {}: {e}", path.display()));
    let Manifest::Events(events) = manifest else {
        panic!("expected events-kind manifest");
    };
    assert_eq!(events.events.len(), 3);
    assert_eq!(
        events.events.get("take"),
        Some(&ManifestEvent {
            args: vec!["int".into(), "string".into()],
        }),
    );
}

#[test]
fn binding_module_combines_with_rename() {
    // The attribute parser handles multiple keys in the same
    // `#[ogham(...)]` group; re-derive a fixture under a renamed
    // record to prove they don't interfere.
    #[derive(OghamState, Clone, PartialEq, Debug)]
    #[ogham(rename = "Renamed", binding_module = "data/ui/x.ogh")]
    struct LocalState {
        v: i32,
    }
    assert_eq!(LocalState::OGHAM_RECORD_NAME, "Renamed");
    let schema = LocalState::ogham_record_schema();
    assert_eq!(schema.fields.len(), 1);
    assert_eq!(
        schema.fields["v"].ty,
        TypeRef::Primitive(PrimType::Int),
    );
}

// ---------------------------------------------------------------------
// Synthesize helper coverage (P0-M4 review fix #1).
//
// The helpers' generic bound requires the OghamState / OghamMsg
// derives, and the derive emits `::ogham::*` paths that only
// resolve from consumer crates. Integration tests in this file are
// the right home; the lib's own `#[cfg(test)]` modules can't run
// these.
// ---------------------------------------------------------------------

#[allow(dead_code)]
#[derive(OghamState, Clone, PartialEq, Debug, Default)]
struct SynthState {
    selected: i32,
    open: bool,
    items: Vec<String>,
    active: Option<i32>,
}

#[allow(dead_code)]
#[derive(OghamMsg, Clone, PartialEq, Debug)]
enum SynthMsg {
    Open,
    Take(i32, String),
    Close,
}

#[allow(dead_code)]
#[derive(OghamState, Clone, PartialEq, Debug, Default)]
struct SynthEmpty {}

#[test]
fn from_state_captures_field_canonical_strings() {
    let manifest = StateManifest::from_state::<SynthState>("data/ui/synth.ogh");
    assert_eq!(manifest.ogh_module, "data/ui/synth.ogh");
    // type_name carries the full module-qualified path; assert the
    // type ident is at the end (the prefix is compiler-dependent).
    assert!(
        manifest.binding.ends_with("SynthState"),
        "got: {}",
        manifest.binding,
    );
    // RustSourceLoc empty in the runtime/synthesized path — see
    // src/typed.rs note. P0-M3 also produces empty values from the
    // proc-macro, so this matches on-disk shape.
    assert_eq!(manifest.rust_source.file, "");
    assert_eq!(manifest.rust_source.line, 0);
    assert_eq!(manifest.host_state.fields.len(), 4);
    assert_eq!(manifest.host_state.fields["selected"].ty, "int");
    assert_eq!(manifest.host_state.fields["open"].ty, "bool");
    assert_eq!(manifest.host_state.fields["items"].ty, "array<string>");
    assert_eq!(manifest.host_state.fields["active"].ty, "int?");
}

#[test]
fn from_state_handles_empty_struct() {
    let manifest = StateManifest::from_state::<SynthEmpty>("test.ogh");
    assert!(manifest.host_state.fields.is_empty());
}

#[test]
fn from_events_captures_arg_canonical_strings() {
    let manifest = EventsManifest::from_events::<SynthMsg>("data/ui/synth.ogh");
    assert_eq!(manifest.ogh_module, "data/ui/synth.ogh");
    assert!(
        manifest.binding.ends_with("SynthMsg"),
        "got: {}",
        manifest.binding,
    );
    assert_eq!(manifest.events.len(), 3);
    assert_eq!(manifest.events["open"].args, Vec::<String>::new());
    assert_eq!(
        manifest.events["take"].args,
        vec!["int".to_string(), "string".to_string()],
    );
    assert_eq!(manifest.events["close"].args, Vec::<String>::new());
}
