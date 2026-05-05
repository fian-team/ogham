//! Phase 1 (M5) — TypedOgham end-to-end tests.
//!
//! Constructs a `TypedOgham<S, M>`, asserts the schema-match
//! check passes/fails as expected, then exercises set_state /
//! poll_msg through the typed surface.

use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::error::RuntimeError;
use ogham::Ogham;
use ogham_derive::{OghamMsg, OghamState};

// ---------------------------------------------------------------------
// Smallest possible smoke test: empty state, two events
// ---------------------------------------------------------------------

#[derive(OghamState, PartialEq, Debug, Clone, Default)]
struct ChestState {}

#[derive(OghamMsg, PartialEq, Debug)]
enum ChestMsg {
    ChestPickUp,
    ChestCancel,
}

// `main` must return a widget — the runtime's bridge layer
// expects one. We emit the event during render so the test can
// observe it via the message queue, then return an empty Flex
// so the bridge is happy.
const CHEST_OGH: &str = r#"
host_state {};
events {
    chest_pick_up(),
    chest_cancel(),
};
let main = fn () {
    event("chest_pick_up");
    Flex { children: [] }
};
"#;

#[test]
fn empty_state_smoke_test_constructs_and_emits_event() {
    let mut typed = Ogham::from_source_typed::<ChestState, ChestMsg>(
        CHEST_OGH,
        ChestState::default(),
        RuntimeConfig::default(),
    )
    .expect("schema must match");
    // The .ogh's `let main` invokes event("chest_pick_up") on
    // the first render. After construction, that handler should
    // have fired and the message should be in the queue.
    let msg = typed.poll_msg();
    assert_eq!(msg, Some(ChestMsg::ChestPickUp));
    assert_eq!(typed.poll_msg(), None, "queue should be empty after drain");
}

// ---------------------------------------------------------------------
// Schema-match: positive
// ---------------------------------------------------------------------

#[derive(OghamState, PartialEq, Debug, Clone, Default)]
struct SettingsState {
    master_volume: String,
    invert_y: bool,
    fov: String,
}

#[derive(OghamMsg, PartialEq, Debug)]
enum SettingsMsg {
    SetMasterVolume(String),
    ToggleInvertY,
    Close,
}

const SETTINGS_OGH: &str = r#"
host_state {
    master_volume: string,
    invert_y: bool,
    fov: string,
};
events {
    set_master_volume(string),
    toggle_invert_y(),
    close(),
};
let main = fn () {
    log master_volume;
    log invert_y;
    log fov;
    Flex { children: [] }
};
"#;

#[test]
fn settings_schema_matches() {
    let typed = Ogham::from_source_typed::<SettingsState, SettingsMsg>(
        SETTINGS_OGH,
        SettingsState {
            master_volume: "0.5".to_string(),
            invert_y: false,
            fov: "75.0".to_string(),
        },
        RuntimeConfig::default(),
    );
    assert!(typed.is_ok(), "expected match, got: {:?}", typed.err());
}

// ---------------------------------------------------------------------
// Schema-match: each negative class
// ---------------------------------------------------------------------

#[test]
fn missing_field_in_rust_fails_with_diff() {
    #[derive(OghamState, PartialEq, Debug, Clone, Default)]
    struct OnlyOne {
        master_volume: String,
        // missing: invert_y, fov
    }
    #[derive(OghamMsg)]
    enum NoEvents {}
    // OghamMsg must have at least Send + 'static; empty enum
    // satisfies that.

    let err = Ogham::from_source_typed::<OnlyOne, SettingsMsg>(
        SETTINGS_OGH,
        OnlyOne::default(),
        RuntimeConfig::default(),
    )
    .err().expect("expected error");
    let RuntimeError::SchemaMismatch(diff) = err else {
        panic!("expected SchemaMismatch, got {:?}", err);
    };
    assert!(diff.contains("invert_y"), "diff: {}", diff);
    assert!(diff.contains("fov"), "diff: {}", diff);
    let _ = std::any::type_name::<NoEvents>();
}

#[test]
fn extra_field_on_rust_fails_with_diff() {
    #[derive(OghamState, PartialEq, Debug, Clone, Default)]
    struct ExtraField {
        master_volume: String,
        invert_y: bool,
        fov: String,
        extra_rust_only: i32,
    }
    let err = Ogham::from_source_typed::<ExtraField, SettingsMsg>(
        SETTINGS_OGH,
        ExtraField::default(),
        RuntimeConfig::default(),
    )
    .err().expect("expected error");
    let RuntimeError::SchemaMismatch(diff) = err else {
        panic!("expected SchemaMismatch, got {:?}", err);
    };
    assert!(diff.contains("extra_rust_only"), "diff: {}", diff);
}

#[test]
fn type_mismatch_on_field_fails_with_diff() {
    #[derive(OghamState, PartialEq, Debug, Clone, Default)]
    struct WrongType {
        master_volume: i32, // .ogh says string
        invert_y: bool,
        fov: String,
    }
    let err = Ogham::from_source_typed::<WrongType, SettingsMsg>(
        SETTINGS_OGH,
        WrongType::default(),
        RuntimeConfig::default(),
    )
    .err().expect("expected error");
    let RuntimeError::SchemaMismatch(diff) = err else {
        panic!("expected SchemaMismatch, got {:?}", err);
    };
    assert!(diff.contains("master_volume"), "diff: {}", diff);
    assert!(diff.contains("type differs"), "diff: {}", diff);
}

#[test]
fn missing_event_on_rust_fails_with_diff() {
    #[derive(OghamMsg, PartialEq, Debug)]
    enum MissingEvent {
        SetMasterVolume(String),
        ToggleInvertY,
        // missing: Close
    }
    let err = Ogham::from_source_typed::<SettingsState, MissingEvent>(
        SETTINGS_OGH,
        SettingsState::default(),
        RuntimeConfig::default(),
    )
    .err().expect("expected error");
    let RuntimeError::SchemaMismatch(diff) = err else {
        panic!("expected SchemaMismatch, got {:?}", err);
    };
    assert!(diff.contains("close"), "diff: {}", diff);
}

#[test]
fn extra_event_on_rust_fails_with_diff() {
    #[derive(OghamMsg, PartialEq, Debug)]
    enum ExtraEvent {
        SetMasterVolume(String),
        ToggleInvertY,
        Close,
        Extra,
    }
    let err = Ogham::from_source_typed::<SettingsState, ExtraEvent>(
        SETTINGS_OGH,
        SettingsState::default(),
        RuntimeConfig::default(),
    )
    .err().expect("expected error");
    let RuntimeError::SchemaMismatch(diff) = err else {
        panic!("expected SchemaMismatch, got {:?}", err);
    };
    assert!(diff.contains("extra"), "diff: {}", diff);
}

#[test]
fn no_host_state_returns_schema_missing_when_rust_state_has_fields() {
    // The Rust state has at least one field, but the module
    // omits `host_state {}` entirely → SchemaMissing.
    // (Event-only modules with empty Rust state are allowed —
    // see `event_only_module_with_empty_state_constructs_ok`.)
    #[derive(OghamState, PartialEq, Debug, Clone, Default)]
    struct StateWithField {
        x: i32,
    }
    #[derive(OghamMsg, PartialEq, Debug)]
    enum AnyMsg {
        Close,
    }
    let err = Ogham::from_source_typed::<StateWithField, AnyMsg>(
        r#"
        events { close() };
        let main = fn () { event("close"); Flex { children: [] } };
        "#,
        StateWithField::default(),
        RuntimeConfig::default(),
    )
    .err()
    .expect("expected error");
    assert!(matches!(err, RuntimeError::SchemaMissing));
}

#[test]
fn event_only_module_with_empty_state_constructs_ok() {
    // chest_ui's exact shape: events declared, no host_state,
    // and an empty Rust state struct. This must succeed —
    // SchemaMissing is over-strict when both sides are empty.
    #[derive(OghamMsg, PartialEq, Debug)]
    enum AnyMsg {
        Close,
    }
    let typed = Ogham::from_source_typed::<ChestState, AnyMsg>(
        r#"
        events { close() };
        let main = fn () { event("close"); Flex { children: [] } };
        "#,
        ChestState::default(),
        RuntimeConfig::default(),
    );
    assert!(typed.is_ok(), "expected ok, got {:?}", typed.err());
}

// ---------------------------------------------------------------------
// set_state / poll_msg interaction
// ---------------------------------------------------------------------

#[test]
fn set_state_diffs_against_previous_snapshot() {
    let mut typed = Ogham::from_source_typed::<SettingsState, SettingsMsg>(
        SETTINGS_OGH,
        SettingsState {
            master_volume: "0.5".to_string(),
            invert_y: false,
            fov: "75.0".to_string(),
        },
        RuntimeConfig::default(),
    )
    .unwrap();

    typed.set_state(SettingsState {
        master_volume: "0.7".to_string(), // changed
        invert_y: false,                  // unchanged
        fov: "75.0".to_string(),          // unchanged
    });

    // (We can't directly observe what was injected without
    // peeking at host_state, but we can confirm the call
    // doesn't crash and the inner Ogham is still operable.)
    let _ = typed.inner();
}

#[test]
fn drain_msgs_iterates_all_pending() {
    let mut typed = Ogham::from_source_typed::<ChestState, ChestMsg>(
        // main fires both events on first render
        r#"
        host_state {};
        events {
            chest_pick_up(),
            chest_cancel(),
        };
        let main = fn () {
            event("chest_pick_up");
            event("chest_cancel");
            Flex { children: [] }
        };
        "#,
        ChestState::default(),
        RuntimeConfig::default(),
    )
    .unwrap();
    let msgs: Vec<_> = typed.drain_msgs().collect();
    assert_eq!(msgs.len(), 2);
}
