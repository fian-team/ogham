//! Phase 1 (M1c) — parser tests for `record`, `host_state`, `events`
//! declarations and the type-ref grammar (`array<T>`, `map<K, V>`,
//! `T?`, `Self`). The schemas in these tests mirror the per-UI
//! schemas in `docs/internal/TYPED_BINDINGS_UL_AUDIT.md` so the
//! grammar stays validated against real-world shapes.

use ogham::parser::typed_bindings::{KeyType, PrimType, TypeRef};
use ogham::parser::{Parser, Statement};
use ogham::scanner::Scanner;

fn parse(source: &str) -> Result<ogham::parser::Function, ogham::parser::SyntaxError> {
    let mut scanner = Scanner::new(source.to_string());
    let tokens = scanner.scan();
    let mut parser = Parser::new(tokens);
    parser.parse()
}

fn parse_ok(source: &str) -> ogham::parser::Function {
    match parse(source) {
        Ok(f) => f,
        Err(err) => panic!("expected source to parse, got error: {:?}", err),
    }
}

fn parse_err(source: &str) -> ogham::parser::SyntaxError {
    match parse(source) {
        Ok(_) => panic!("expected parse error, got success for source:\n{}", source),
        Err(e) => e,
    }
}

// ---------------------------------------------------------------------
// Top-level declarations
// ---------------------------------------------------------------------

#[test]
fn empty_host_state_block_parses() {
    let module = parse_ok("host_state {};");
    let stmts = &module.body.statement_list;
    assert_eq!(stmts.len(), 1);
    match &stmts[0] {
        Statement::HostStateDeclaration(decl) => assert!(decl.fields.is_empty()),
        other => panic!("expected HostStateDeclaration, got {:?}", other),
    }
}

#[test]
fn empty_events_block_parses() {
    let module = parse_ok("events {};");
    match &module.body.statement_list[0] {
        Statement::EventsDeclaration(decl) => assert!(decl.events.is_empty()),
        other => panic!("expected EventsDeclaration, got {:?}", other),
    }
}

#[test]
fn empty_record_parses() {
    let module = parse_ok("record Empty {};");
    match &module.body.statement_list[0] {
        Statement::RecordDeclaration(decl) => {
            assert_eq!(decl.name, "Empty");
            assert!(decl.fields.is_empty());
        }
        other => panic!("expected RecordDeclaration, got {:?}", other),
    }
}

// ---------------------------------------------------------------------
// Type-ref grammar
// ---------------------------------------------------------------------

#[test]
fn primitives_parse() {
    let module = parse_ok(
        "host_state {
          a: int,
          b: float,
          c: string,
          d: bool,
        };",
    );
    let Statement::HostStateDeclaration(decl) = &module.body.statement_list[0] else {
        panic!("expected HostStateDeclaration");
    };
    assert_eq!(decl.fields[0].ty, TypeRef::Primitive(PrimType::Int));
    assert_eq!(decl.fields[1].ty, TypeRef::Primitive(PrimType::Float));
    assert_eq!(decl.fields[2].ty, TypeRef::Primitive(PrimType::String));
    assert_eq!(decl.fields[3].ty, TypeRef::Primitive(PrimType::Bool));
}

#[test]
fn array_of_primitive() {
    let module = parse_ok("host_state { items: array<string> };");
    let Statement::HostStateDeclaration(decl) = &module.body.statement_list[0] else {
        panic!()
    };
    assert_eq!(
        decl.fields[0].ty,
        TypeRef::Array(Box::new(TypeRef::Primitive(PrimType::String)))
    );
}

#[test]
fn nested_array_of_array() {
    // Mirrors dm_inventory_ui's `rows: array<array<Cell>>`.
    let module = parse_ok("host_state { rows: array<array<Cell>> };");
    let Statement::HostStateDeclaration(decl) = &module.body.statement_list[0] else {
        panic!()
    };
    assert_eq!(
        decl.fields[0].ty,
        TypeRef::Array(Box::new(TypeRef::Array(Box::new(TypeRef::Record(
            "Cell".to_string()
        )))))
    );
}

#[test]
fn map_string_to_string() {
    // Mirrors settings_ui's `keybinds: map<string, string>`.
    let module = parse_ok("host_state { keybinds: map<string, string> };");
    let Statement::HostStateDeclaration(decl) = &module.body.statement_list[0] else {
        panic!()
    };
    assert_eq!(
        decl.fields[0].ty,
        TypeRef::Map(KeyType::String, Box::new(TypeRef::Primitive(PrimType::String)))
    );
}

#[test]
fn map_int_key_parses() {
    let module = parse_ok("host_state { lookup: map<int, string> };");
    let Statement::HostStateDeclaration(decl) = &module.body.statement_list[0] else {
        panic!()
    };
    assert_eq!(
        decl.fields[0].ty,
        TypeRef::Map(KeyType::Int, Box::new(TypeRef::Primitive(PrimType::String)))
    );
}

#[test]
fn map_with_invalid_key_type_errors() {
    let err = parse_err("host_state { bad: map<float, int> };");
    assert!(err.message.contains("not a valid map key type"));
    assert!(err.note.is_some());
}

#[test]
fn optional_postfix_wraps_type() {
    let module = parse_ok("host_state { name: string? };");
    let Statement::HostStateDeclaration(decl) = &module.body.statement_list[0] else {
        panic!()
    };
    assert_eq!(
        decl.fields[0].ty,
        TypeRef::Optional(Box::new(TypeRef::Primitive(PrimType::String)))
    );
}

#[test]
fn double_optional_chains() {
    let module = parse_ok("host_state { weird: int?? };");
    let Statement::HostStateDeclaration(decl) = &module.body.statement_list[0] else {
        panic!()
    };
    assert_eq!(
        decl.fields[0].ty,
        TypeRef::Optional(Box::new(TypeRef::Optional(Box::new(TypeRef::Primitive(
            PrimType::Int
        )))))
    );
}

#[test]
fn array_of_optional_record() {
    let module = parse_ok("host_state { selections: array<Item?> };");
    let Statement::HostStateDeclaration(decl) = &module.body.statement_list[0] else {
        panic!()
    };
    assert_eq!(
        decl.fields[0].ty,
        TypeRef::Array(Box::new(TypeRef::Optional(Box::new(TypeRef::Record(
            "Item".to_string()
        )))))
    );
}

#[test]
fn record_reference_is_unresolved() {
    let module = parse_ok("host_state { player: PlayerInfo };");
    let Statement::HostStateDeclaration(decl) = &module.body.statement_list[0] else {
        panic!()
    };
    // The parser leaves it as a Record(name); the resolver checks
    // existence later.
    assert_eq!(decl.fields[0].ty, TypeRef::Record("PlayerInfo".to_string()));
}

// ---------------------------------------------------------------------
// `Self` placement
// ---------------------------------------------------------------------

#[test]
fn self_inside_record_parses() {
    let module = parse_ok(
        "record Tree {
          children: array<Self>,
          parent: Self?,
        };",
    );
    let Statement::RecordDeclaration(decl) = &module.body.statement_list[0] else {
        panic!()
    };
    assert_eq!(
        decl.fields[0].ty,
        TypeRef::Array(Box::new(TypeRef::SelfRef))
    );
    assert_eq!(
        decl.fields[1].ty,
        TypeRef::Optional(Box::new(TypeRef::SelfRef))
    );
}

#[test]
fn self_in_host_state_errors() {
    let err = parse_err("host_state { weird: Self };");
    assert!(err.message.contains("Self"));
    assert!(err.note.is_some());
}

// ---------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------

#[test]
fn integer_default_parses() {
    use ogham::parser::typed_bindings::SchemaLiteral;
    let module = parse_ok("host_state { count: int = 42 };");
    let Statement::HostStateDeclaration(decl) = &module.body.statement_list[0] else {
        panic!()
    };
    assert_eq!(decl.fields[0].default, Some(SchemaLiteral::Int(42)));
}

#[test]
fn negative_integer_default_parses() {
    use ogham::parser::typed_bindings::SchemaLiteral;
    let module = parse_ok("host_state { offset: int = -7 };");
    let Statement::HostStateDeclaration(decl) = &module.body.statement_list[0] else {
        panic!()
    };
    assert_eq!(decl.fields[0].default, Some(SchemaLiteral::Int(-7)));
}

#[test]
fn float_string_bool_defaults_parse() {
    use ogham::parser::typed_bindings::SchemaLiteral;
    let module = parse_ok(
        r#"host_state {
              x: float = 1.5,
              greeting: string = "hi",
              flag: bool = true,
            };"#,
    );
    let Statement::HostStateDeclaration(decl) = &module.body.statement_list[0] else {
        panic!()
    };
    assert_eq!(decl.fields[0].default, Some(SchemaLiteral::Float(1.5)));
    assert_eq!(
        decl.fields[1].default,
        Some(SchemaLiteral::String("hi".to_string()))
    );
    assert_eq!(decl.fields[2].default, Some(SchemaLiteral::Bool(true)));
}

#[test]
fn non_literal_default_errors() {
    let err = parse_err("host_state { x: int = some_var };");
    assert!(err.note.is_some());
}

// ---------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------

#[test]
fn no_arg_event_parses() {
    let module = parse_ok("events { close() };");
    let Statement::EventsDeclaration(decl) = &module.body.statement_list[0] else {
        panic!()
    };
    assert_eq!(decl.events[0].name, "close");
    assert!(decl.events[0].args.is_empty());
}

#[test]
fn multi_arg_event_parses() {
    // Mirrors social_ui's `accept_faction_request(string, int)`.
    let module = parse_ok("events { accept_faction_request(string, int) };");
    let Statement::EventsDeclaration(decl) = &module.body.statement_list[0] else {
        panic!()
    };
    let ev = &decl.events[0];
    assert_eq!(ev.name, "accept_faction_request");
    assert_eq!(ev.args.len(), 2);
    assert_eq!(ev.args[0], TypeRef::Primitive(PrimType::String));
    assert_eq!(ev.args[1], TypeRef::Primitive(PrimType::Int));
}

#[test]
fn events_with_record_args_parse() {
    let module = parse_ok(
        "events {
            apply_color(RgbColor),
            move_item(Item, int),
        };",
    );
    let Statement::EventsDeclaration(decl) = &module.body.statement_list[0] else {
        panic!()
    };
    assert_eq!(decl.events.len(), 2);
    assert_eq!(decl.events[0].args[0], TypeRef::Record("RgbColor".to_string()));
    assert_eq!(decl.events[1].args[0], TypeRef::Record("Item".to_string()));
}

#[test]
fn trailing_commas_allowed_everywhere() {
    let _ = parse_ok(
        r#"
        record Foo { a: int, b: string, };
        host_state { x: int, y: string, };
        events { close(), do(int, string,), };
        "#,
    );
}

// ---------------------------------------------------------------------
// Uniqueness / placement
// ---------------------------------------------------------------------

#[test]
fn duplicate_host_state_errors() {
    let err = parse_err(
        "host_state { a: int };
         host_state { b: int };",
    );
    assert!(err.message.contains("duplicate `host_state`"));
    assert!(err.note.is_some());
}

#[test]
fn duplicate_events_errors() {
    let err = parse_err(
        "events { foo() };
         events { bar() };",
    );
    assert!(err.message.contains("duplicate `events`"));
}

#[test]
fn multiple_records_allowed() {
    let _ = parse_ok(
        "record A { x: int };
         record B { y: string };
         record C { z: bool };",
    );
}

#[test]
fn record_inside_function_body_errors() {
    let err = parse_err(
        "let f = fn () {
            record Inner { x: int };
            5
         };",
    );
    assert!(err.message.contains("only allowed at module top level"));
}

#[test]
fn host_state_inside_function_body_errors() {
    let err = parse_err(
        "let f = fn () {
            host_state { x: int };
            5
         };",
    );
    assert!(err.message.contains("module top level"));
}

// ---------------------------------------------------------------------
// Loose mode preserved (no schema declarations = no behavior change)
// ---------------------------------------------------------------------

#[test]
fn loose_mode_module_still_parses() {
    let _ = parse_ok(
        r#"
        let counter = fn () {
            state count = 0;
            count
        };
        let main = fn () {
            counter()
        };
        "#,
    );
}

// ---------------------------------------------------------------------
// Audit-derived schemas (real-world fixtures from
// TYPED_BINDINGS_UL_AUDIT.md). These prove the grammar handles
// production shapes.
// ---------------------------------------------------------------------

#[test]
fn audit_chest_ui_schema_parses() {
    let _ = parse_ok(
        "events {
            chest_pick_up(),
            chest_cancel(),
        };",
    );
}

#[test]
fn audit_tip_log_ui_schema_parses() {
    let _ = parse_ok(
        r#"
        record TipEntry { title: string, body: string };
        host_state {
            tips: array<TipEntry>,
            tip_count: int,
        };
        events { close_tip_log() };
        "#,
    );
}

#[test]
fn audit_crafting_ui_schema_parses() {
    let _ = parse_ok(
        r#"
        record CraftingRecipe {
            id: string,
            name: string,
            inputs_text: string,
            can_craft: bool,
        };
        host_state {
            station_name: string,
            recipes: array<CraftingRecipe>,
            is_pending: bool,
        };
        events {
            craft_recipe(string),
            close_crafting(),
        };
        "#,
    );
}

#[test]
fn audit_dm_hud_optional_record_parses() {
    let _ = parse_ok(
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
    );
}

#[test]
fn audit_talents_nested_records_parse() {
    let _ = parse_ok(
        r#"
        record SkillRequirement { text: string, met: bool };
        record TalentCard {
            id: string,
            name: string,
            description: string,
            requirements: array<SkillRequirement>,
            meets_requirements: bool,
            owned: bool,
            can_learn: bool,
            blocked_reason: string,
        };
        host_state {
            char_level: int,
            talent_points_available: int,
            respec_points_available: int,
            talents: array<TalentCard>,
        };
        events {
            close_talents(),
            learn_talent(string),
            respec_talent(string),
        };
        "#,
    );
}

#[test]
fn audit_dm_inventory_nested_arrays_parse() {
    let _ = parse_ok(
        r#"
        record Cell {
            cx: int, cy: int,
            kind: string,
            name: string, w: int, h: int,
        };
        host_state {
            target_name: string,
            target_kind: string,
            inv_width: int, inv_height: int,
            rows: array<array<Cell>>,
            grant_buffer: string,
        };
        events {
            dm_inv_close(),
            dm_inv_take(int, int),
            dm_inv_grant_input(string),
            dm_inv_grant_submit(),
        };
        "#,
    );
}
