//! `screen` declarations and scoped host state.
//!
//! Routing's ogham half: a document declares its routable surfaces, the
//! host injects a path, and the document renders that path and nothing
//! else. The three claims worth testing are the three that make it worth
//! having at all:
//!
//! - **The path selects the view.** Nothing in the document decides what
//!   is on screen, and there is no `mode` string to drift against a
//!   router (`ROUTING.md` axiom 1).
//! - **A screen's slice is its own.** Two screens may declare a field of
//!   the same name and neither can read the other's — which is the whole
//!   of what replaces a union state struct spanning every screen
//!   (axiom 5).
//! - **Ids are checked against the route table at load.** A registered id
//!   with no block, or a block nobody routes to, is an error naming both
//!   (axiom 11).
//!
//! The tests read the module's returned `Value` rather than laying out a
//! widget tree: routing decides *which* view is built, and that decision
//! is visible in the descriptor. Layout is not part of the claim.

use ogham::parser::Parser;
use ogham::runtime::compiler::Compiler;
use ogham::runtime::error::VMError;
use ogham::runtime::schema::ModuleSchema;
use ogham::runtime::value::Value;
use ogham::runtime::Runtime;
use ogham::scanner::Scanner;

// ── helpers ────────────────────────────────────────────────────────────

/// A document with three screens, each with its own slice. Two of them
/// deliberately declare a field called `label`, which is the collision a
/// union struct cannot express.
const THREE_SCREENS: &str = r#"
host_state {
  heading: string = "",
};

screen "title" {
  state { label: string = "" }
  view Text { content: "title/" + label + "/" + heading }
};

screen "world" {
  state { label: string = "", rows: int = 0 }
  view Text { content: "world/" + label + "/" + rows }
};

screen "journal" {
  view Text { content: "journal/" + heading }
};

let main = fn () { outlet() };
"#;

fn runtime(source: &str) -> Runtime {
    Runtime::from_source(source, None).expect("from_source")
}

fn render(rt: &mut Runtime) -> Value {
    let module = rt.get_module().expect("module").clone();
    rt.execute_module(&module).expect("execute")
}

/// The `content` of every `Text` in the tree, in tree order. Enough to
/// say which screens rendered and what each of them could see.
fn texts(value: &Value) -> Vec<String> {
    let mut out = Vec::new();
    collect_texts(value, &mut out);
    out
}

fn collect_texts(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Widget(w) => {
            if w.identifier.get() == "Text" {
                if let Some(Value::String(s)) = w.properties.get("content") {
                    out.push(s.clone());
                }
            }
            for (_, v) in &w.properties {
                collect_texts(v, out);
            }
        }
        Value::Array(items) => {
            for v in items {
                collect_texts(v, out);
            }
        }
        _ => {}
    }
}

fn compile(source: &str) -> Result<(), VMError> {
    let tokens = Scanner::new(source.to_string()).scan();
    let module = Parser::new(tokens).parse().expect("parse should succeed");
    Compiler::compile_module(&module).map(|_| ())
}

fn schema_of(source: &str) -> ModuleSchema {
    let tokens = Scanner::new(source.to_string()).scan();
    let module = Parser::new(tokens).parse().expect("parse should succeed");
    ModuleSchema::from_module(&module).expect("schema should resolve")
}

// ── the path selects the view ──────────────────────────────────────────

#[test]
fn the_injected_path_selects_the_screen() {
    let mut rt = runtime(THREE_SCREENS);
    rt.set_screen_state("title", "label", "T");
    rt.set_screen_state("world", "label", "W");
    rt.set_host_state("heading", "H");

    rt.set_route_path(&["title"]);
    assert_eq!(texts(&render(&mut rt)), vec!["title/T/H"]);

    rt.set_route_path(&["world"]);
    assert_eq!(texts(&render(&mut rt)), vec!["world/W/0"]);

    rt.set_route_path(&["journal"]);
    assert_eq!(texts(&render(&mut rt)), vec!["journal/H"]);
}

#[test]
fn an_empty_path_renders_no_screen() {
    // Not an error and not a fallback screen: before the first frame, or
    // between two of them, there is genuinely nothing routed.
    let mut rt = runtime(THREE_SCREENS);
    rt.set_route_path::<&str>(&[]);
    assert!(texts(&render(&mut rt)).is_empty());
}

#[test]
fn an_unrouted_id_in_the_path_renders_nothing_rather_than_failing() {
    // The load-time check below is what catches a table/document
    // mismatch. At render time a stray id must not take the frame down —
    // a blank surface is recoverable, a panic mid-frame is not.
    let mut rt = runtime(THREE_SCREENS);
    rt.set_route_path(&["nowhere"]);
    assert!(texts(&render(&mut rt)).is_empty());
}

#[test]
fn a_deeper_route_renders_over_a_shallower_one() {
    // The path is a stack, rendered outermost first. Which ids survive
    // occlusion is the router's decision, made before injection — the
    // document just draws what it is given, in order.
    let mut rt = runtime(THREE_SCREENS);
    rt.set_screen_state("world", "label", "W");
    rt.set_host_state("heading", "H");
    rt.set_route_path(&["world", "journal"]);
    assert_eq!(texts(&render(&mut rt)), vec!["world/W/0", "journal/H"]);
}

#[test]
fn a_screen_is_presences_child_with_no_wrapper() {
    // The outlet emits exactly the shape every consumer hand-wrote:
    // `Presence { key, children: [ <the screen> ] }`. A wrapper is what a
    // stack of two visible views would need, and adding one cost the entry
    // animations — an absolutely-positioned layer sets the child's paint
    // transform, so a widget whose `initial` also sets one snapped to its
    // final place while its ghost still faded out.
    let mut rt = runtime(THREE_SCREENS);
    rt.set_route_path(&["title"]);
    let tree = render(&mut rt);
    let Value::Widget(root) = &tree else {
        panic!("the outlet renders a widget, got {tree:?}")
    };
    assert_eq!(root.identifier.get(), "Presence");
    let Some(Value::Array(children)) = root.properties.get("children") else {
        panic!("Presence takes children")
    };
    assert_eq!(children.len(), 1);
    let Value::Widget(child) = &children[0] else {
        panic!("the screen is the child")
    };
    assert_eq!(
        child.identifier.get(),
        "Text",
        "the screen's own view is Presence's direct child — no layer between"
    );
}

#[test]
fn the_path_is_also_published_as_one_key() {
    // `Presence` sequences on a scalar key. Without it the outlet swaps
    // its child in place and any keyed widget with an `exit` animation
    // plays it *in layout flow*, so the outgoing and incoming pages push
    // each other around as they cross-fade.
    let mut rt = runtime(THREE_SCREENS);
    rt.set_route_path(&["world", "journal"]);
    assert_eq!(
        rt.get_host_state("__route_key"),
        Some(Value::String("world/journal".to_string()))
    );
    rt.set_route_path::<&str>(&[]);
    assert_eq!(
        rt.get_host_state("__route_key"),
        Some(Value::String(String::new()))
    );
}

// ── a screen's slice is its own ─────────────────────────────────────────

#[test]
fn two_screens_may_declare_the_same_field_and_neither_sees_the_other() {
    let mut rt = runtime(THREE_SCREENS);
    rt.set_screen_state("title", "label", "mine");
    rt.set_screen_state("world", "label", "theirs");
    rt.set_host_state("heading", "");

    rt.set_route_path(&["title"]);
    assert_eq!(texts(&render(&mut rt)), vec!["title/mine/"]);

    rt.set_route_path(&["world"]);
    assert_eq!(texts(&render(&mut rt)), vec!["world/theirs/0"]);
}

#[test]
fn a_screen_reads_the_root_scope_too() {
    // Chrome-global keys — the measured remainder that does not partition
    // per route — stay in `host_state {}` and every screen sees them.
    let mut rt = runtime(THREE_SCREENS);
    rt.set_host_state("heading", "shared");
    rt.set_route_path(&["journal"]);
    assert_eq!(texts(&render(&mut rt)), vec!["journal/shared"]);
}

#[test]
fn a_screens_private_field_is_not_a_name_anywhere_else() {
    // `rows` belongs to "world". Naming it from "title" is not a silent
    // read of the neighbour's value — it is an unknown identifier, which
    // is the property that makes the slices worth having.
    let err = compile(
        r#"
host_state { heading: string = "" };
screen "world" { state { rows: int = 0 } view Text { content: "w" } };
screen "title" { view Text { content: rows } };
let main = fn () { outlet() };
"#,
    )
    .expect_err("reading another screen's field must not compile");
    let VMError::StrictMode(err) = err else {
        panic!("expected a strict-mode error, got {err:?}");
    };
    assert!(
        err.message.contains("unknown identifier `rows`"),
        "unexpected message: {}",
        err.message
    );
}

#[test]
fn a_screen_field_shadows_nothing_when_it_is_not_declared() {
    // A name a screen did not declare falls through to the root scope,
    // which is what makes `state {}` optional rather than a wrapper every
    // screen has to write.
    let mut rt = runtime(THREE_SCREENS);
    rt.set_host_state("heading", "root");
    rt.set_route_path(&["journal"]);
    assert_eq!(texts(&render(&mut rt)), vec!["journal/root"]);
}

// ── declaration rules ──────────────────────────────────────────────────

#[test]
fn a_screen_needs_a_view() {
    let tokens = Scanner::new(
        r#"screen "title" { state { a: int = 0 } };
let main = fn () { outlet() };"#
            .to_string(),
    )
    .scan();
    let err = Parser::new(tokens)
        .parse()
        .expect_err("a screen with no view must not parse");
    assert!(
        err.message.contains("declares no `view`"),
        "unexpected message: {}",
        err.message
    );
}

#[test]
fn a_duplicate_screen_id_is_rejected() {
    let tokens = Scanner::new(
        r#"screen "title" { view Text { content: "a" } };
screen "title" { view Text { content: "b" } };
let main = fn () { outlet() };"#
            .to_string(),
    )
    .scan();
    let err = Parser::new(tokens)
        .parse()
        .expect_err("two screens may not share an id");
    assert!(
        err.message.contains("duplicate screen id `title`"),
        "unexpected message: {}",
        err.message
    );
}

#[test]
fn a_screen_id_may_contain_characters_an_identifier_may_not() {
    // Route ids are route ids, not ogham identifiers: `map-edit` is a
    // legal id and would scan as three tokens if screens were named by
    // their id rather than by index.
    let mut rt = runtime(
        r#"
screen "map-edit" { state { n: int = 0 } view Text { content: "edit/" + n } };
let main = fn () { outlet() };
"#,
    );
    rt.set_screen_state("map-edit", "n", 7);
    rt.set_route_path(&["map-edit"]);
    assert_eq!(texts(&render(&mut rt)), vec!["edit/7"]);
}

#[test]
fn a_screen_is_only_a_declaration_at_module_top_level() {
    // Inside a function body `screen` is an ordinary identifier, so this
    // is a syntax error rather than a nested declaration — which is the
    // point: the declaration form is narrow enough that no other use of
    // the name can be mistaken for it.
    let tokens = Scanner::new(
        r#"let main = fn () { screen "x" { view Text { content: "a" } }; };"#.to_string(),
    )
    .scan();
    assert!(
        Parser::new(tokens).parse().is_err(),
        "a nested screen must not parse as a declaration"
    );
}

#[test]
fn screen_is_still_usable_as_an_ordinary_name() {
    // This is not hypothetical. Making `screen` a keyword broke three
    // shipped documents on the first run across the repos: celia has a
    // `screen(width, children)` layout helper and regency a `screen`
    // host-state field, and both failed as "Expected identifier"
    // pointing at a line that had not changed in months.
    let mut rt = runtime(
        r#"
host_state { screen: string = "" };
let screen = fn (label: string) { Text { content: "in " + label } };
screen "world" { view screen("world") };
let main = fn () { outlet() };
"#,
    );
    rt.set_route_path(&["world"]);
    assert_eq!(texts(&render(&mut rt)), vec!["in world"]);
}

// ── the table and the document are checked against each other ──────────

#[test]
fn a_registered_id_with_no_screen_block_is_a_load_error() {
    let schema = schema_of(THREE_SCREENS);
    let err = schema
        .validate_screens(&["title", "world", "journal", "wardrobe"])
        .expect_err("a registered id with no block must fail");
    assert!(
        err.contains("registered with no `screen` block: wardrobe"),
        "unexpected message: {err}"
    );
}

#[test]
fn a_screen_block_nobody_routes_to_is_a_load_error() {
    let schema = schema_of(THREE_SCREENS);
    let err = schema
        .validate_screens(&["title", "world"])
        .expect_err("an unrouted block must fail");
    assert!(
        err.contains("declared but not registered: journal"),
        "unexpected message: {err}"
    );
}

#[test]
fn a_matching_table_and_document_validate() {
    let schema = schema_of(THREE_SCREENS);
    schema
        .validate_screens(&["journal", "title", "world"])
        .expect("a matching table must validate");
}

#[test]
fn a_document_with_no_screens_is_vacuously_valid() {
    // Documents that predate routing, and fragments a host mounts whole,
    // declare none — and must not be forced to.
    let schema = schema_of(r#"let main = fn () { Text { content: "x" } };"#);
    schema
        .validate_screens(&["title"])
        .expect("a screenless document must validate");
}

// ── the rest of the language is unmoved ────────────────────────────────

#[test]
fn a_document_without_screens_still_compiles_and_renders() {
    let mut rt = runtime(r#"let main = fn () { Text { content: "plain" } };"#);
    assert_eq!(texts(&render(&mut rt)), vec!["plain"]);
}

#[test]
fn view_is_still_usable_as_an_ordinary_name() {
    // `view` is contextual, not a keyword: taking it from every document
    // in every repo would be a cost the feature does not need to impose.
    let mut rt = runtime(
        r#"let view = "not a keyword";
let main = fn () { Text { content: view } };"#,
    );
    assert_eq!(texts(&render(&mut rt)), vec!["not a keyword"]);
}
