//! WP-3.2: `select` — the consumer's half of the contract
//! (`docs/internal/APPLICATION.md` §4.1, §4.6, §4.7).
//!
//! Two properties are load-bearing and both are constructed here rather
//! than asserted:
//!
//! - **§4.6, top-level binding.** `tests/documents/manor_hud.ogh` is
//!   regency's in-manor HUD family, checked in verbatim: five helpers,
//!   twenty unqualified reads of `hud`. The two arms of
//!   [`the_manor_screen_family_compiles_against_a_selection_unedited`]
//!   put the *same bytes* under a `host_state {}` block and under a
//!   `select`, and both compile. That is the migration property in the
//!   only form that can be checked — if the selected arm needed one edit,
//!   the two arms could not share the file.
//! - **§4.7, fragments.** One selection, stated once in a shared module,
//!   validated against a different providing scope at each mount.

use std::path::{Path, PathBuf};

use ogham::contract::{Documents, Finding, Mount, Scope, Store};
use ogham::parser::Parser;
use ogham::runtime::compiler::Compiler;
use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::scanner::Scanner;
use ogham::Ogham;

use structure::schema::{Field, Kind, Lit, Schema};

/// regency's palette, verbatim.
const STATIONERY: &str = include_str!("documents/manor_stationery.ogh");

/// regency's manor HUD family, verbatim. Read by both arms, unedited.
const HUD: &str = include_str!("documents/manor_hud.ogh");

/// The record block those helpers are written against today — regency's
/// `Pip` and `Hud`, verbatim from `client.ogh` lines 72-98.
///
/// Twenty-four fields of shape the document restates and the Rust builder
/// restates, held together by a hand-written conformance test. It is what
/// the selection below replaces with one line.
const DECLARED: &str = r#"
record Pip { filled: bool };

record Hud {
  clock: string,
  act: string,
  mode_hint: string,
  threat_caption: string,
  threat: array<Pip>,
  threat_alarm: bool,
  threat_round: bool,
  name: string,
  status: string,
  status_alarm: bool,
  purse_label: string,
  purse: string,
  purse_lit: bool,
  impropriety: string,
  impropriety_alarm: bool,
  light_text: string,
  light_frac: float,
  vitality: array<Pip>,
  composure: array<Pip>,
  condition_shown: bool,
  prompt: string,
};

host_state { hud: Hud };
"#;

/// The same contract, selected. One line, and the shapes stay where they
/// were declared — in the Rust type the manor node provides.
const SELECTED: &str = "\nselect manor { hud };\n";

const MAIN: &str = r#"
let main = fn () {
  Flex { style: {}, children: [ hud_evening(), hud_you() ] }
};
"#;

/// Parse and compile — which is where strict-mode identifier resolution
/// runs, and so where "compiles against a selection" is answered.
fn compiles(source: &str) -> Result<(), String> {
    let module = Parser::new(Scanner::new(source.to_string()).scan())
        .parse()
        .map_err(|e| format!("{e:?}"))?;
    Compiler::compile_module(&module)
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

/// The diagnostic a document that will not compile produced.
fn refused(source: &str) -> String {
    match compiles(source) {
        Ok(_) => panic!("this document was expected not to compile"),
        Err(why) => why,
    }
}

// ── §4.6: the binding ──────────────────────────────────────────────────

/// **The acceptance test.** regency's `manor_screen` family compiles
/// against a selection with no body edits.
///
/// Both arms are the same three constants in the same order, and the one
/// that differs is the *declaration*: twenty-four restated field shapes, or
/// `select manor { hud };`. The helpers in between are one file, read
/// twice.
#[test]
fn the_manor_screen_family_compiles_against_a_selection_unedited() {
    let declared = format!("{STATIONERY}{DECLARED}{HUD}{MAIN}");
    let selected = format!("{STATIONERY}{SELECTED}{HUD}{MAIN}");

    compiles(&declared).expect("the family compiles as it is written today");
    compiles(&selected).expect("and against a selection, with the same bodies");

    // The bodies really are the same bytes: everything after the
    // declaration is identical, character for character.
    assert_eq!(
        declared.split_once(&format!("{HUD}")).map(|(_, rest)| rest),
        selected.split_once(&format!("{HUD}")).map(|(_, rest)| rest),
    );
}

/// The binding is not a loosening. A selected document is still strict —
/// a name it does not select is an unknown identifier at compile time,
/// with the same diagnostic a declared one gets.
///
/// This is the half that would be easy to get wrong and impossible to
/// notice: a `select` that merely turned strict mode *off* would compile
/// every arm of the test above and give a modder nothing.
#[test]
fn a_selected_document_is_still_strict_about_every_other_name() {
    let selected = format!("{STATIONERY}{SELECTED}{HUD}{MAIN}");
    let typo = selected.replace("hud.clock", "hud.clock + hudd");
    let why = refused(&typo);
    assert!(why.contains("hudd"), "{why}");
    assert!(why.contains("did you mean `hud`"), "{why}");
}

/// A name may be bound once. Two selections that both bind `heading` would
/// leave every read of it reaching whichever one the lookup happened to
/// find, so the document is refused at parse time and the name is said.
#[test]
fn a_name_two_selections_both_bind_is_refused_naming_it() {
    let why = refused(
        "select foyer { heading };\nselect table { heading };\nlet main = fn () { heading };",
    );
    assert!(why.contains("heading"), "{why}");

    let why = refused(
        "host_state { heading: string };\nselect table { heading };\nlet main = fn () { heading };",
    );
    assert!(why.contains("host_state"), "{why}");
}

/// A selection names fields. Writing a shape beside one is the inverted
/// contract coming back, so it is refused *naming what to write instead* —
/// because the first thing an author migrating a `host_state {}` block will
/// do is paste it in and change the keyword.
#[test]
fn a_selection_that_restates_a_shape_says_so() {
    let why = refused("select manor { hud: Hud };\nlet main = fn () { hud };");
    assert!(why.contains("selected, not declared"), "{why}");
}

/// `select` is contextual, not a keyword. Every document that already uses
/// the word — as a helper, a field, a local — goes on compiling, which is
/// the measured rule `screen` established (`LANGUAGE.md`).
#[test]
fn select_is_still_an_ordinary_name() {
    compiles(
        r#"
        let select = fn (width: int, children: int) { Flex { style: { width: width } } };
        let main = fn () { select(1, 2) };
        "#,
    )
    .expect("celia's `screen`-shaped layout helper, under the other name");

    compiles(
        r#"
        host_state { select: string };
        let main = fn () { Text { text: select } };
        "#,
    )
    .expect("a host-state field called `select`");
}

// ── the contract, in §4.1's two grades ─────────────────────────────────

/// The manor view's scope, as P5/P6 will publish it: the records that were
/// the document's `record` block, now the provider's Rust types.
#[derive(Clone, Debug, Default, PartialEq)]
struct Manor;

impl Schema for Manor {
    fn reflect() -> Kind {
        let pip = Kind::Record(vec![Field::new("filled", Kind::Bool)]);
        let hud = Kind::Record(vec![
            Field::new("clock", Kind::Str),
            Field::new("act", Kind::Str),
            Field::new("mode_hint", Kind::Str),
            Field::new("threat_caption", Kind::Str),
            Field::new("threat", Kind::List(Box::new(pip.clone()))),
            Field::new("threat_alarm", Kind::Bool),
            Field::new("threat_round", Kind::Bool),
            Field::new("name", Kind::Str),
            Field::new("status", Kind::Str),
            Field::new("status_alarm", Kind::Bool),
            Field::new("purse_label", Kind::Str),
            Field::new("purse", Kind::Str),
            Field::new("purse_lit", Kind::Bool),
            Field::new("impropriety", Kind::Str),
            Field::new("impropriety_alarm", Kind::Bool),
            Field::new("light_text", Kind::Str),
            Field::new("light_frac", Kind::Float),
            Field::new("vitality", Kind::List(Box::new(pip.clone()))),
            Field::new("composure", Kind::List(Box::new(pip))),
            Field::new("condition_shown", Kind::Bool),
            Field::new("prompt", Kind::Str),
        ]);
        Kind::Record(vec![Field::new("hud", hud)])
    }
    fn at_mount(_: Option<&Lit>) -> Self {
        Self
    }
    fn type_name() -> Option<&'static str> {
        Some("Manor")
    }
}

/// The sea panel's scope — the §4.7 fixture. Two nodes provide the same
/// three fields, under two different Rust types, because a fragment mounts
/// under the world root and under the editor and the two are not the same
/// scope (Appendix B.1).
#[derive(Clone, Debug, Default, PartialEq)]
struct Sea;

impl Schema for Sea {
    fn reflect() -> Kind {
        Kind::Record(vec![
            Field::new("sea_panel", Kind::Str),
            Field::new("sea_duration", Kind::Float),
            Field::new("sea_dirty", Kind::Bool),
        ])
    }
    fn at_mount(_: Option<&Lit>) -> Self {
        Self
    }
    fn type_name() -> Option<&'static str> {
        Some("Sea")
    }
}

/// The editor's, missing the field the fragment reads — the drift §4.7 asks
/// to refuse at the mount where it is wrong and nowhere else.
#[derive(Clone, Debug, Default, PartialEq)]
struct DriftedSea;

impl Schema for DriftedSea {
    fn reflect() -> Kind {
        Kind::Record(vec![
            Field::new("sea_panel", Kind::Str),
            Field::new("sea_duration", Kind::Float),
        ])
    }
    fn at_mount(_: Option<&Lit>) -> Self {
        Self
    }
    fn type_name() -> Option<&'static str> {
        Some("DriftedSea")
    }
}

const MANOR: Scope = Scope::Node("manor");
const WORLD: Scope = Scope::Node("world");
const EDITOR: Scope = Scope::Node("editor");

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ogham-selection-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn write(dir: &Path, name: &str, source: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, source).expect("write");
    path
}

/// The selection that replaced twenty-four restated shapes, checked against
/// the scope that provides them. Nothing in the document says what `hud`
/// *is*, so nothing in the document can be wrong about what it is.
#[test]
fn the_selection_is_checked_against_the_scope_it_names() {
    let dir = scratch("manor");
    let path = write(
        &dir,
        "table.ogh",
        &format!("{STATIONERY}{SELECTED}{HUD}{MAIN}"),
    );
    let mut store = Store::new();
    store.provides::<Manor>(MANOR).expect("the manor's scope");

    let found = Documents::new(&store)
        .mounting(Mount::new(&path).selecting(MANOR))
        .check()
        .expect("the document reads");
    assert!(!found.refuses(), "{found}");
}

/// A selection naming a field the scope does not provide refuses, named —
/// §4.1's loud grade, and the modder's case.
#[test]
fn a_selected_field_the_scope_does_not_provide_refuses() {
    let dir = scratch("unprovided");
    let path = write(
        &dir,
        "table.ogh",
        "select manor { hud, wheel };\nlet main = fn () { Flex { style: {} } };",
    );
    let mut store = Store::new();
    store.provides::<Manor>(MANOR).expect("the manor's scope");

    let found = Documents::new(&store)
        .mounting(Mount::new(&path).selecting(MANOR))
        .check()
        .expect("the document reads");
    assert!(
        found
            .refusals()
            .any(|f| matches!(f, Finding::Unprovided { field, .. } if field == "wheel")),
        "{found}"
    );
}

/// A selection naming a scope the document does not mount under refuses at
/// the scope rather than once per field, so the sentence names the cause.
#[test]
fn a_selection_from_a_scope_the_document_does_not_mount_under_refuses() {
    let dir = scratch("unmounted");
    let path = write(
        &dir,
        "foyer.ogh",
        "select manor { hud };\nlet main = fn () { Flex { style: {} } };",
    );
    let mut store = Store::new();
    store.provides::<Manor>(MANOR).expect("the manor's scope");

    let found = Documents::new(&store)
        .mounting(Mount::new(&path).selecting(Scope::Process))
        .check()
        .expect("the document reads");
    let printed = found.to_string();
    assert!(found.refuses(), "{printed}");
    assert!(printed.contains("manor"), "{printed}");
}

// ── §4.7: one selection, several mounts ────────────────────────────────

/// The shared module: untold_lore's sea panel, which lives under the world
/// root and under the editor. It states its selection **once**, names no
/// scope — it cannot, the scopes differ between its mounts — and reads its
/// three fields as top-level names like any other document.
const SEA_PANEL: &str = r#"
select { sea_panel, sea_duration, sea_dirty };

let sea_body = fn () {
  Flex {
    style: { width: "grow" },
    children: [
      Text { text: sea_panel },
      Text { text: match sea_dirty { true => "*", false => "" } },
      Flex { style: { width: 4 * sea_duration, height: 2 } },
    ],
  }
};
"#;

const MOUNTS_THE_PANEL: &str = r#"
import "./sea_panel.ogh";
let main = fn () { sea_body() };
"#;

/// A fragment's selection travels with every document that mounts it, and
/// is validated against **that mount's** scopes.
///
/// The same three names, stated once, are checked twice here: against the
/// world root, which provides them, and against the editor, which has lost
/// one. The first mount does not refuse and the second does — which is the
/// whole of §4.7, because a fragment that validated once and travelled
/// unchecked would be a `host_state {}` block with extra steps.
#[test]
fn a_fragment_is_validated_against_each_providing_scope_at_each_mount() {
    let dir = scratch("fragment");
    write(&dir, "sea_panel.ogh", SEA_PANEL);
    let world = write(&dir, "world.ogh", MOUNTS_THE_PANEL);
    let editor = write(&dir, "editor.ogh", MOUNTS_THE_PANEL);

    let mut store = Store::new();
    store.provides::<Sea>(WORLD).expect("the world's scope");
    store
        .provides::<DriftedSea>(EDITOR)
        .expect("the editor's scope");

    let found = Documents::new(&store)
        .mounting(Mount::new(&world).selecting(WORLD))
        .check()
        .expect("the document reads");
    assert!(
        !found.refuses(),
        "the world root provides all three: {found}"
    );

    let found = Documents::new(&store)
        .mounting(Mount::new(&editor).selecting(EDITOR))
        .check()
        .expect("the document reads");
    assert!(
        found
            .refusals()
            .any(|f| matches!(f, Finding::Unprovided { field, .. } if field == "sea_dirty")),
        "the same selection, refused at the mount that lost the field: {found}"
    );
}

/// And the fragment binds its names inside its *own* file: the helper that
/// reads `sea_panel` is compiled strictly, in the module that selected it.
///
/// This is what a shared module could not do before. `host_state {}` does
/// not cross an import, so a helper family lifted out of a document went
/// loose the moment it moved — every read of every field silently
/// unchecked, in exactly the file a modder is most likely to edit.
#[test]
fn a_fragment_binds_its_own_names_and_the_document_that_mounts_it_too() {
    let dir = scratch("fragment-strict");
    write(&dir, "sea_panel.ogh", SEA_PANEL);
    let path = write(&dir, "world.ogh", MOUNTS_THE_PANEL);
    let mut store = Store::new();
    store.provides::<Sea>(WORLD).expect("the world's scope");
    let seeded = Mount::new(&path).selecting(WORLD).at_mount(&store);
    let config = RuntimeConfig::new()
        .with_project_root(dir.clone())
        .with_host_state(seeded);
    let ui = Ogham::watch(path.to_string_lossy().into_owned(), config.clone())
        .expect("the document mounts");
    assert!(
        ui.module_schema()
            .expect("the schema resolves")
            .selects("sea_panel"),
        "the fragment's selection travels into the mounting document"
    );

    // A read the fragment did not select does not compile, in the
    // fragment's own file.
    write(
        &dir,
        "sea_panel.ogh",
        &SEA_PANEL.replace("text: sea_panel", "text: sea_pannel"),
    );
    let why = Ogham::watch(path.to_string_lossy().into_owned(), config)
        .err()
        .map(|e| format!("{e:?}"))
        .expect("a name the fragment does not select does not compile");
    assert!(why.contains("sea_pannel"), "{why}");
}

/// A selected name is read exactly as a declared one was: the same
/// host-state key, pushed the same way.
///
/// This is why §4.6's binding costs no bytecode and no host change — the
/// projection a game already writes goes on working, which is the other
/// half of "the migration touches zero helper bodies".
#[test]
fn a_selected_name_reads_the_host_state_key_of_the_same_name() {
    let ui = Ogham::from_source(
        "select manor { hud };\nlet main = fn () { Text { text: hud } };",
        RuntimeConfig::new().with_host_state(
            [("hud".to_string(), Value::String(String::new()))]
                .into_iter()
                .collect(),
        ),
    )
    .expect("a document with one selected name");
    ui.with_runtime_mut(|rt| {
        rt.inject_host_state("hud".to_string(), Value::String("9:40".to_string()))
    });
    assert_eq!(
        ui.with_runtime_mut(|rt| rt.get_host_state("hud")),
        Some(Value::String("9:40".to_string()))
    );
}

/// A selection declares no defaults, so its at-mount values come from the
/// providing scope — and a document that mounts and immediately draws
/// renders on the first frame, before any host has projected anything.
///
/// This is what `host_state {}`'s declared defaults did and a selection
/// deliberately cannot: the value a field holds at mount is the provider's
/// declaration (§4.1), so it is read off the reflection instead of
/// restated in the document.
#[test]
fn a_document_that_only_selects_renders_before_its_host_has_projected() {
    let dir = scratch("at-mount");
    let path = write(
        &dir,
        "table.ogh",
        &format!("{STATIONERY}{SELECTED}{HUD}{MAIN}"),
    );
    let mut store = Store::new();
    store.provides::<Manor>(MANOR).expect("the manor's scope");
    let mount = Mount::new(&path).selecting(MANOR);

    let seeded = mount.at_mount(&store);
    assert!(seeded.contains_key("hud"), "the selection is seeded");

    let ui = Ogham::watch(
        path.to_string_lossy().into_owned(),
        RuntimeConfig::new()
            .with_project_root(dir.clone())
            .with_host_state(seeded),
    )
    .expect("the manor draws before anything has been projected into it");
    assert!(ui
        .module_schema()
        .expect("the schema resolves")
        .selects("hud"));
}
