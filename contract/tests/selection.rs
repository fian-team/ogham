//! WP-3.2: what a `select` is checked **against**
//! (`../docs/internal/APPLICATION.md` §4.1, §4.6, §4.7), and how far into
//! what it names a document is allowed to read.
//!
//! Three properties:
//!
//! - **§4.1's two grades**, over a selection: a selected field the scope
//!   does not provide refuses, named.
//! - **§4.7, fragments.** One selection, stated once in a shared module,
//!   validated against a different providing scope at each mount.
//! - **Leaf depth.** A selection carries names only, because shapes are
//!   the provider's declaration — so what stops `hud.clock` reading
//!   `Void` after a provider renames the field is this harness walking the
//!   document's reads and resolving each one through the reflection.
//!
//! The language half — that regency's helper bodies compile unedited
//! against a selection — is `ogham/tests/selection.rs`, because the
//! compiler answers it with no store at all.

use std::path::{Path, PathBuf};

use contract::{Documents, Finding, Mount, Scope, Store};
use ogham::runtime::config::RuntimeConfig;
use ogham::Ogham;

use structure::schema::{Field, Kind, Lit, Schema};

/// regency's palette, verbatim.
const STATIONERY: &str = include_str!("../../tests/documents/manor_stationery.ogh");

/// regency's manor HUD family, verbatim — five helpers and twenty
/// unqualified reads of `hud`, most of them one level deep.
const HUD: &str = include_str!("../../tests/documents/manor_hud.ogh");

/// The whole contract, in one line. The shapes stay where they were
/// declared: in the Rust type the manor node provides.
const SELECTED: &str = "\nselect manor { hud };\n";

const MAIN: &str = r#"
let main = fn () {
  Flex { style: {}, children: [ hud_evening(), hud_you() ] }
};
"#;

// ── the contract, in §4.1's two grades ─────────────────────────────────

/// The manor's HUD shape, as P5/P6 will publish it: the records that were
/// the document's `record` block, now the provider's Rust types.
///
/// Parameterised by two things, because the leaf-depth tests below are
/// exactly about a provider that changed one of them: what `clock` is
/// called, and what a pip is made of.
fn hud_kind(clock: &str, pip: Kind) -> Kind {
    Kind::Record(vec![Field::new(
        "hud",
        Kind::Record(vec![
            Field::new(clock, Kind::Str),
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
        ]),
    )])
}

/// A pip as regency's document reads it.
fn pip_kind() -> Kind {
    Kind::Record(vec![Field::new("filled", Kind::Bool)])
}

/// The manor view's scope, whole and agreeing.
#[derive(Clone, Debug, Default, PartialEq)]
struct Manor;

impl Schema for Manor {
    fn reflect() -> Kind {
        hud_kind("clock", pip_kind())
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

// ── leaf depth: how far into a selected name a document may read ───────

/// The manor's scope after a provider-side rename: `Hud::clock` is
/// `Hud::time_of_day` now, and nobody touched the document.
#[derive(Clone, Debug, Default, PartialEq)]
struct RenamedManor;

impl Schema for RenamedManor {
    fn reflect() -> Kind {
        hud_kind("time_of_day", pip_kind())
    }
    fn at_mount(_: Option<&Lit>) -> Self {
        Self
    }
    fn type_name() -> Option<&'static str> {
        Some("RenamedManor")
    }
}

/// The manor's scope after a rename the harness **cannot** see: a pip has
/// no `filled` any more.
///
/// Every read of it in the document goes through something the walk
/// cannot resolve — `hud.vitality[i].filled` indexes a list, and
/// `hud_pip`'s body reads `p.filled` off its own parameter — so the
/// honest answer is silence.
#[derive(Clone, Debug, Default, PartialEq)]
struct PiplessManor;

impl Schema for PiplessManor {
    fn reflect() -> Kind {
        hud_kind("clock", Kind::Record(vec![Field::new("lit", Kind::Bool)]))
    }
    fn at_mount(_: Option<&Lit>) -> Self {
        Self
    }
    fn type_name() -> Option<&'static str> {
        Some("PiplessManor")
    }
}

fn manor_document(dir: &Path) -> PathBuf {
    write(
        dir,
        "table.ogh",
        &format!("{STATIONERY}{SELECTED}{HUD}{MAIN}"),
    )
}

/// **The acceptance test.** A provider renames a field a shipped document
/// reads through a bound name, and the harness refuses, naming it.
///
/// This is the guarantee the inversion cost and this package puts back.
/// Under `host_state { hud: Hud }` the document restated twenty-four field
/// shapes, so a renamed `clock` stopped two copies of the shape agreeing
/// and refused. `select manor { hud }` restates nothing — which is right,
/// shapes are the provider's — and so the selection alone still holds:
/// `hud` is provided. Only the *reads* say `clock`, and only the harness
/// has both the reads and the reflection.
///
/// The document is regency's real HUD family, unedited.
#[test]
fn a_field_the_provider_renamed_refuses_where_the_document_reads_it() {
    let dir = scratch("renamed");
    let path = manor_document(&dir);

    let mut store = Store::new();
    store
        .provides::<RenamedManor>(MANOR)
        .expect("the manor's scope");

    let found = Documents::new(&store)
        .mounting(Mount::new(&path).selecting(MANOR))
        .check()
        .expect("the document reads");

    assert!(
        found.refuses(),
        "`hud.clock` reads nothing now, and reading nothing quietly is the \
         one thing §4.1 promises never to happen: {found}"
    );
    let refusal = found
        .refusals()
        .find(|f| matches!(f, Finding::Unreached { field, .. } if field == "hud.clock"))
        .expect("the refusal names the field the provider renamed");
    assert!(refusal.to_string().contains("hud.clock"), "{refusal}");

    // And the selection itself is not what refused: `hud` is provided.
    assert!(
        !found
            .refusals()
            .any(|f| matches!(f, Finding::Unprovided { .. })),
        "the name is fine; it is the depth that is not: {found}"
    );
}

/// **The other acceptance test.** A read the harness cannot resolve
/// statically produces no refusal at all — not a report either.
///
/// A false refusal is worse than the gap being closed: it would refuse a
/// document that is perfectly correct, and the check would be turned off
/// within a week. So the walk stops where it stops and says nothing about
/// what is past it.
///
/// Both unresolvable shapes are in regency's real file, which is why it is
/// the fixture: `hud.vitality[i].filled` steps into a list (§4.2 — a
/// collection is one field in v1), and `hud_pip` reads `p.filled` off a
/// parameter, which has no declared shape to resolve against. The pip's
/// `filled` is renamed out from under both here, and the harness is
/// silent about both.
#[test]
fn a_read_the_harness_cannot_resolve_is_silent_rather_than_refused() {
    let dir = scratch("unresolvable");
    let path = manor_document(&dir);

    let mut store = Store::new();
    store
        .provides::<PiplessManor>(MANOR)
        .expect("the manor's scope");

    let found = Documents::new(&store)
        .mounting(Mount::new(&path).selecting(MANOR))
        .check()
        .expect("the document reads");

    assert!(
        !found.refuses(),
        "nothing here is resolvable to a leaf, so nothing here is refusable: {found}"
    );
    assert!(
        !found
            .all()
            .iter()
            .any(|f| matches!(f, Finding::Unreached { .. })),
        "and it is not downgraded to a report either — an unseeable read is \
         not a finding: {found}"
    );
}

/// A read that stops *at* a collection is a read like any other, and it
/// still refuses when the collection is not there.
///
/// The line §4.2 draws is "a collection is one field", not "anything near
/// a collection is unknowable": `hud.threat` resolves to a list and is
/// checked; `hud.threat[i].filled` is not.
#[test]
fn a_read_that_stops_at_a_collection_is_still_checked() {
    let dir = scratch("collection");
    let path = write(
        &dir,
        "table.ogh",
        "select manor { hud };\n\
         let main = fn () { Flex { style: {}, children: for (i in 0..hud.thret.length()) { i } } };",
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
            .any(|f| matches!(f, Finding::Unreached { field, .. } if field == "hud.thret")),
        "{found}"
    );
}

/// A fragment's reads cross the import with it.
///
/// §4.7's shared module states its selection once and its helpers live in
/// its own file, so a check that read only the mounting document would
/// hold this guarantee exactly nowhere useful — a helper family lifted out
/// of a document is where a modder edits, and it is the file the
/// selection is *not* in.
#[test]
fn a_fragments_reads_are_checked_at_the_mount_like_its_selection() {
    let dir = scratch("fragment-reads");
    write(
        &dir,
        "panel.ogh",
        "select { hud };\nlet body = fn () { Text { text: hud.clock } };\n",
    );
    let path = write(
        &dir,
        "world.ogh",
        "import \"./panel.ogh\";\nlet main = fn () { body() };\n",
    );

    let mut store = Store::new();
    store.provides::<Manor>(MANOR).expect("a whole manor");
    store
        .provides::<RenamedManor>(WORLD)
        .expect("and one that renamed the field");

    let found = Documents::new(&store)
        .mounting(Mount::new(&path).selecting(MANOR))
        .check()
        .expect("the document reads");
    assert!(!found.refuses(), "the manor still has a `clock`: {found}");

    let found = Documents::new(&store)
        .mounting(Mount::new(&path).selecting(WORLD))
        .check()
        .expect("the document reads");
    assert!(
        found
            .refusals()
            .any(|f| matches!(f, Finding::Unreached { field, .. } if field == "hud.clock")),
        "the same read, refused at the mount that lost the field: {found}"
    );
}

/// The same question, at the other moment. [`Documents`] is `cargo test`
/// time; `contract::refusals` is what a mount's load and every hot reload
/// ask (§4.1: "validation runs at document load **and at every hot
/// reload**"). A grade only one of them held would be a grade every hot
/// session escapes until the next restart.
#[test]
fn the_load_and_reload_gate_asks_the_same_question_at_leaf_depth() {
    let dir = scratch("gate-depth");
    let path = manor_document(&dir);
    let schema = ogham::runtime::schema::load_schema(&path).expect("the document reads");

    let mut store = Store::new();
    store
        .provides::<RenamedManor>(MANOR)
        .expect("the manor's scope");

    let why = contract::refusals(&schema, &store, &Mount::new(&path).selecting(MANOR))
        .expect_err("the gate refuses the candidate");
    assert!(why.contains("hud.clock"), "{why}");
}
