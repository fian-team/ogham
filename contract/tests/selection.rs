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
