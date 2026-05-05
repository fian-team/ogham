//! Phase 1 (M4) — derive macro smoke tests.
//!
//! Validates that `#[derive(OghamRecord)]`, `#[derive(OghamState)]`,
//! and `#[derive(OghamMsg)]` produce compiling, behaviorally-correct
//! impls against real-shaped fixtures.
//!
//! These tests don't yet exercise `TypedOgham::watch_typed` (M5)
//! — they prove the derive output works against the schema /
//! event APIs in isolation.

use std::collections::HashMap;

use ogham::runtime::host_state::HostStateSink;
use ogham::runtime::schema::{
    KeyType, ModuleSchema, OghamField, OghamMsg, OghamRecord, OghamState, PrimType, TypeRef,
};
use ogham::runtime::value::Value;
use ogham_derive::{OghamMsg, OghamRecord, OghamState};

// ---------------------------------------------------------------------
// OghamRecord — primitive fields
// ---------------------------------------------------------------------

#[derive(OghamRecord, PartialEq, Debug)]
struct SimpleItem {
    name: String,
    count: i32,
    available: bool,
}

#[test]
fn simple_record_schema_is_correct() {
    assert_eq!(SimpleItem::OGHAM_RECORD_NAME, "SimpleItem");
    let schema = SimpleItem::ogham_record_schema();
    assert_eq!(schema.fields.len(), 3);
    assert_eq!(
        schema.fields["name"].ty,
        TypeRef::Primitive(PrimType::String)
    );
    assert_eq!(
        schema.fields["count"].ty,
        TypeRef::Primitive(PrimType::Int)
    );
    assert_eq!(
        schema.fields["available"].ty,
        TypeRef::Primitive(PrimType::Bool)
    );
}

#[test]
fn simple_record_into_value_produces_map() {
    let item = SimpleItem {
        name: "ore".to_string(),
        count: 7,
        available: true,
    };
    let v = item.into_ogham_value();
    let Value::Map(map) = v else {
        panic!("expected Value::Map, got {:?}", v);
    };
    assert_eq!(map["name"], Value::String("ore".to_string()));
    assert_eq!(map["count"], Value::Integer(7));
    assert_eq!(map["available"], Value::Boolean(true));
}

// ---------------------------------------------------------------------
// OghamRecord — nested + container types
// ---------------------------------------------------------------------

#[derive(OghamRecord, PartialEq, Debug)]
struct Inventory {
    items: Vec<SimpleItem>,
    selected: Option<SimpleItem>,
    nicknames: HashMap<String, String>,
}

#[test]
fn nested_record_schema_resolves_to_container_types() {
    let schema = Inventory::ogham_record_schema();
    assert_eq!(
        schema.fields["items"].ty,
        TypeRef::Array(Box::new(TypeRef::Record("SimpleItem".to_string())))
    );
    assert_eq!(
        schema.fields["selected"].ty,
        TypeRef::Optional(Box::new(TypeRef::Record("SimpleItem".to_string())))
    );
    assert_eq!(
        schema.fields["nicknames"].ty,
        TypeRef::Map(KeyType::String, Box::new(TypeRef::Primitive(PrimType::String)))
    );
}

#[test]
fn optional_none_serializes_as_void() {
    let inv = Inventory {
        items: vec![],
        selected: None,
        nicknames: HashMap::new(),
    };
    let v = inv.into_ogham_value();
    let Value::Map(map) = v else { panic!() };
    assert_eq!(map["selected"], Value::Void);
}

#[test]
fn optional_some_serializes_as_inner() {
    let inv = Inventory {
        items: vec![],
        selected: Some(SimpleItem {
            name: "axe".to_string(),
            count: 1,
            available: true,
        }),
        nicknames: HashMap::new(),
    };
    let v = inv.into_ogham_value();
    let Value::Map(map) = v else { panic!() };
    let Value::Map(inner) = &map["selected"] else {
        panic!("expected nested map for Some(SimpleItem)");
    };
    assert_eq!(inner["name"], Value::String("axe".to_string()));
}

#[test]
fn array_of_records_serializes() {
    let inv = Inventory {
        items: vec![
            SimpleItem {
                name: "a".to_string(),
                count: 1,
                available: true,
            },
            SimpleItem {
                name: "b".to_string(),
                count: 2,
                available: false,
            },
        ],
        selected: None,
        nicknames: HashMap::new(),
    };
    let v = inv.into_ogham_value();
    let Value::Map(map) = v else { panic!() };
    let Value::Array(items) = &map["items"] else {
        panic!("expected Value::Array")
    };
    assert_eq!(items.len(), 2);
}

// ---------------------------------------------------------------------
// OghamRecord — attribute customization
// ---------------------------------------------------------------------

#[derive(OghamRecord)]
#[ogham(rename = "Player")]
struct PlayerInternal {
    #[ogham(rename = "display_name")]
    name: String,
    #[ogham(skip)]
    runtime_only: i32,
}

#[test]
fn record_rename_attribute_overrides_type_name() {
    assert_eq!(PlayerInternal::OGHAM_RECORD_NAME, "Player");
}

#[test]
fn field_rename_attribute_overrides_field_name() {
    let schema = PlayerInternal::ogham_record_schema();
    assert!(schema.fields.contains_key("display_name"));
    assert!(!schema.fields.contains_key("name"));
}

#[test]
fn field_skip_attribute_omits_field_from_schema_and_value() {
    let schema = PlayerInternal::ogham_record_schema();
    assert!(!schema.fields.contains_key("runtime_only"));
    let p = PlayerInternal {
        name: "rowan".to_string(),
        runtime_only: 999,
    };
    let v = p.into_ogham_value();
    let Value::Map(map) = v else { panic!() };
    assert!(!map.contains_key("runtime_only"));
}

// ---------------------------------------------------------------------
// OghamState — snapshot + diff
// ---------------------------------------------------------------------

#[derive(OghamState, PartialEq, Debug, Clone)]
struct GameUiState {
    score: i32,
    player_name: String,
    is_paused: bool,
}

#[test]
fn ogham_state_snapshot_pushes_all_fields() {
    let state = GameUiState {
        score: 42,
        player_name: "rowan".to_string(),
        is_paused: false,
    };
    let mut sink: HashMap<String, Value> = HashMap::new();
    OghamState::ogham_snapshot_into(&state, &mut sink);
    assert_eq!(sink["score"], Value::Integer(42));
    assert_eq!(sink["player_name"], Value::String("rowan".to_string()));
    assert_eq!(sink["is_paused"], Value::Boolean(false));
}

#[test]
fn ogham_state_diff_only_pushes_changed_fields() {
    let prev = GameUiState {
        score: 42,
        player_name: "rowan".to_string(),
        is_paused: false,
    };
    let next = GameUiState {
        score: 43,
        player_name: "rowan".to_string(),
        is_paused: false,
    };
    let mut sink: HashMap<String, Value> = HashMap::new();
    OghamState::ogham_diff_apply(&next, &prev, &mut sink);
    assert_eq!(sink.len(), 1, "only `score` should be diffed");
    assert_eq!(sink["score"], Value::Integer(43));
}

#[test]
fn ogham_state_diff_no_changes_no_writes() {
    let prev = GameUiState {
        score: 0,
        player_name: "x".to_string(),
        is_paused: true,
    };
    let mut sink: HashMap<String, Value> = HashMap::new();
    OghamState::ogham_diff_apply(&prev, &prev, &mut sink);
    assert!(sink.is_empty());
}

// ---------------------------------------------------------------------
// OghamMsg — variant decoding
// ---------------------------------------------------------------------

#[derive(OghamMsg, PartialEq, Debug)]
enum SettingsMsg {
    Close,
    SetMasterVolume(String),
    Rebind(String, String),
    SelectIndex(i32),
}

#[test]
fn ogham_msg_unit_variant_round_trips() {
    let parsed = SettingsMsg::try_from_ogham_event("close", &[]).unwrap();
    assert_eq!(parsed, SettingsMsg::Close);
}

#[test]
fn ogham_msg_single_arg_variant_round_trips() {
    let v = vec![Value::String("0.5".to_string())];
    let parsed = SettingsMsg::try_from_ogham_event("set_master_volume", &v).unwrap();
    assert_eq!(parsed, SettingsMsg::SetMasterVolume("0.5".to_string()));
}

#[test]
fn ogham_msg_multi_arg_variant_round_trips() {
    let v = vec![
        Value::String("jump".to_string()),
        Value::String("Space".to_string()),
    ];
    let parsed = SettingsMsg::try_from_ogham_event("rebind", &v).unwrap();
    assert_eq!(
        parsed,
        SettingsMsg::Rebind("jump".to_string(), "Space".to_string())
    );
}

#[test]
fn ogham_msg_unknown_event_returns_none() {
    let parsed = SettingsMsg::try_from_ogham_event("nope", &[]);
    assert!(parsed.is_none());
}

#[test]
fn ogham_msg_wrong_arity_returns_none() {
    let parsed = SettingsMsg::try_from_ogham_event(
        "set_master_volume",
        &[Value::String("a".to_string()), Value::String("b".to_string())],
    );
    assert!(parsed.is_none());
}

#[test]
fn ogham_msg_int_arg_round_trips() {
    let v = vec![Value::Integer(7)];
    let parsed = SettingsMsg::try_from_ogham_event("select_index", &v).unwrap();
    assert_eq!(parsed, SettingsMsg::SelectIndex(7));
}

#[test]
fn ogham_msg_events_listing_includes_all_variants() {
    let events = SettingsMsg::ogham_events();
    assert!(events.contains_key("close"));
    assert!(events.contains_key("set_master_volume"));
    assert!(events.contains_key("rebind"));
    assert!(events.contains_key("select_index"));
    let rebind_sig = &events["rebind"];
    assert_eq!(rebind_sig.args.len(), 2);
    assert_eq!(rebind_sig.args[0], TypeRef::Primitive(PrimType::String));
}

#[test]
fn ogham_msg_rename_attribute_overrides_event_name() {
    #[derive(OghamMsg)]
    #[allow(dead_code)]
    enum Renamed {
        #[ogham(rename = "custom_close_name")]
        Close,
    }
    let events = Renamed::ogham_events();
    assert!(events.contains_key("custom_close_name"));
    assert!(!events.contains_key("close"));
}

// ---------------------------------------------------------------------
// Schema-match scenarios — the audit's chest_ui & dm_hud fixtures
// ---------------------------------------------------------------------

#[derive(OghamState, PartialEq, Debug, Clone, Default)]
struct ChestState {}

#[derive(OghamMsg, PartialEq, Debug)]
enum ChestMsg {
    ChestPickUp,
    ChestCancel,
}

#[test]
fn audit_chest_ui_derives_compile_and_match_an_empty_module() {
    // The .ogh side: no host_state fields, two events.
    let parsed = ogham::runtime::schema::load_schema_from_source(
        r#"
        events {
            chest_pick_up(),
            chest_cancel(),
        };
        let main = fn () {
            event("chest_pick_up");
        };
        "#,
    )
    .unwrap();
    // ChestState's schema is empty; matches the absent host_state.
    let derived = ChestState::ogham_record_schema();
    assert!(derived.fields.is_empty());
    assert!(parsed.host_state.is_none());

    // ChestMsg's events match parsed events.
    let derived_events = ChestMsg::ogham_events();
    assert_eq!(derived_events.len(), 2);
    for k in derived_events.keys() {
        assert!(parsed.events.contains_key(k), "missing parsed event: {}", k);
    }
}

#[derive(OghamRecord, PartialEq, Debug, Clone)]
struct EntityInspector {
    name: String,
    kind: String,
    position_text: String,
    detail_lines: Vec<String>,
    can_possess: bool,
    is_possessing: bool,
    can_open_inventory: bool,
}

#[derive(OghamState, PartialEq, Debug, Clone, Default)]
struct DmHudState {
    paused: bool,
    selection_count: i32,
    selected_entity: Option<EntityInspector>,
}

#[derive(OghamMsg)]
enum DmHudMsg {
    DmTogglePause,
    DmOpenInventory,
    DmPossess,
    DmRelease,
    DmDeselect,
}

#[test]
fn audit_dm_hud_derives_match_module_schema_shape() {
    let parsed = ogham::runtime::schema::load_schema_from_source(
        r#"
        record EntityInspector {
            name: string,
            kind: string,
            position_text: string,
            detail_lines: array<string>,
            can_possess: bool,
            is_possessing: bool,
            can_open_inventory: bool,
        };
        host_state {
            paused: bool,
            selection_count: int,
            selected_entity: EntityInspector?,
        };
        events {
            dm_toggle_pause(),
            dm_open_inventory(),
            dm_possess(),
            dm_release(),
            dm_deselect(),
        };
        "#,
    )
    .unwrap();

    let derived_state = DmHudState::ogham_record_schema();
    let parsed_state = parsed.host_state.as_ref().unwrap();
    assert_eq!(derived_state.fields.len(), parsed_state.fields.len());
    for (name, parsed_field) in &parsed_state.fields {
        let derived_field = derived_state
            .fields
            .get(name)
            .unwrap_or_else(|| panic!("derived state missing field {}", name));
        assert_eq!(
            derived_field.ty, parsed_field.ty,
            "type mismatch on field `{}`",
            name
        );
    }
    let derived_events = DmHudMsg::ogham_events();
    assert_eq!(derived_events.len(), parsed.events.len());

    // Avoid the "unused" warnings on _DmHudMsg variants.
    let _ = ModuleSchema::default();
}
