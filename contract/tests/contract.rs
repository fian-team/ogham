//! WP-2.4: the two grades, the harness that asks them at `cargo test`
//! time, and the hot-reload refusal that leaves the running instance
//! standing (`docs/internal/APPLICATION.md` §4.1).
//!
//! The fixtures are celia's, because celia is the consumer whose live
//! drift §4.1 names by hand: a root `status` the host computes every frame
//! and nothing reads, and a `back()` in the document's `events {}` block
//! that reached nobody for months.

use std::path::{Path, PathBuf};

use contract::{Checked, Documents, Finding, Mount, Scope, Store};
use ogham::route::Chrome;
use ogham::runtime::config::RuntimeConfig;
use ogham::Ogham;

use structure::intent::{Accepted, Intents, Parameter, Raise, Refused, Vocabulary};
use structure::schema::{Field, Kind, Lit, Schema};

// ── the provider's side ────────────────────────────────────────────────
//
// Written out by hand rather than derived, because `#[derive(Schema)]`
// lives in lorekeeper and this repo cannot reach it. What a consumer
// writes is one `#[derive(Schema)]` and one `#[derive(Intent)]`; what it
// hands the harness is the same `Store` either way.

/// The engine's front-of-house scope, as P5 will publish it.
#[derive(Clone, Debug, Default, PartialEq)]
struct Front {
    heading: String,
    status: String,
}

impl Schema for Front {
    fn reflect() -> Kind {
        Kind::Record(vec![
            Field::new("heading", Kind::Str),
            Field::new("status", Kind::Str).absent_when("no connection is being made"),
        ])
    }
    fn at_mount(_: Option<&Lit>) -> Self {
        Self::default()
    }
    fn type_name() -> Option<&'static str> {
        Some("Front")
    }
}

/// One roster tile — the record shape a document redeclares in its own
/// language, and which therefore has to meet this one **structurally**
/// (§4.7): two lists of named fields, never two names.
#[derive(Clone, Debug, Default, PartialEq)]
struct Tile {
    id: String,
    name: String,
    focused: bool,
}

impl Schema for Tile {
    fn reflect() -> Kind {
        Kind::Record(vec![
            Field::new("id", Kind::Str),
            Field::new("name", Kind::Str),
            Field::new("focused", Kind::Bool),
        ])
    }
    fn at_mount(_: Option<&Lit>) -> Self {
        Self::default()
    }
    fn type_name() -> Option<&'static str> {
        Some("Tile")
    }
}

/// The lobby view's scope, flat rather than nested (B.3's record-grain
/// lesson).
#[derive(Clone, Debug, Default, PartialEq)]
struct Lobby {
    roster: Vec<Tile>,
    can_confirm: bool,
}

impl Schema for Lobby {
    fn reflect() -> Kind {
        Kind::Record(vec![
            Field::new("roster", Kind::List(Box::new(Tile::reflect()))),
            Field::new("can_confirm", Kind::Bool),
        ])
    }
    fn at_mount(_: Option<&Lit>) -> Self {
        Self::default()
    }
    fn type_name() -> Option<&'static str> {
        Some("Lobby")
    }
}

/// The lobby's intents. `withdraw` is the one a provider publishes that no
/// shipped document raises — the licensed modding surface §4.1 refuses to
/// refuse.
#[derive(Clone, Debug, PartialEq)]
enum Muster {
    Pick { character: String },
    Confirm,
    Withdraw,
}

impl Intents for Muster {
    fn vocabulary() -> Vocabulary {
        Vocabulary::new(vec![
            Accepted::new("pick", vec![Parameter::of::<String>("character")]),
            Accepted::new("confirm", vec![]),
            Accepted::new("withdraw", vec![]),
        ])
    }

    fn accept(raise: &Raise) -> Result<Self, Refused> {
        match raise.name() {
            "pick" => {
                let mut p = raise.parameters("pick", 1)?;
                Ok(Muster::Pick {
                    character: p.take::<String>("character")?,
                })
            }
            "confirm" => {
                raise.parameters("confirm", 0)?;
                Ok(Muster::Confirm)
            }
            "withdraw" => {
                raise.parameters("withdraw", 0)?;
                Ok(Muster::Withdraw)
            }
            other => Err(Refused::NoSuchIntent {
                intent: other.to_string(),
            }),
        }
    }
}

const FRONT: Scope = Scope::Process;
const LOBBY: Scope = Scope::Node("lobby");

/// A consumer's registration function, whole. No path, no mount, no frame,
/// no window — which is exactly why the harness answers under `cargo test`
/// rather than at first boot.
fn store() -> Store {
    let mut store = Store::new();
    store.provides::<Front>(FRONT).expect("the process scope");
    store.provides::<Lobby>(LOBBY).expect("the lobby's scope");
    store.accepts::<Muster>(LOBBY).expect("the lobby's intents");
    store
}

// ── the consumer's side ────────────────────────────────────────────────

/// A shipped document that holds its whole contract: it selects only
/// fields the two scopes provide, raises only intents the lobby accepts,
/// and draws exactly the screen the table registers.
const HOLDS: &str = r#"
record Tile { id: string, name: string, focused: bool };

host_state {
  heading: string,
  status: string,
};

events {
  pick(string),
  confirm(),
  withdraw(),
};

screen "lobby" {
  state { roster: array<Tile>, can_confirm: bool }
  view Flex { style: {} }
};

let main = fn () { outlet() };
"#;

fn shipped(tag: &str, source: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ogham-contract-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("doc.ogh");
    std::fs::write(&path, source).expect("write");
    path
}

/// The mount a consumer declares: this document, these scopes nearest
/// first, these registered ids. The mapping is an input, because which
/// scope a document selects against is the binding's answer to give and
/// the binding lands in P4.
fn mount(path: &Path) -> Mount {
    Mount::new(path)
        .selecting(LOBBY)
        .selecting(FRONT)
        .drawing(&["lobby"])
}

fn check(tag: &str, source: &str) -> structure::Findings {
    let path = shipped(tag, source);
    Documents::new(&store())
        .mounting(mount(&path))
        .check()
        .expect("the shipped document reads")
}

// ── the CI moment ──────────────────────────────────────────────────────

/// The whole harness, over a document that agrees: no refusals, and the
/// only reports are the coverage §4.1 licenses.
///
/// This one test is what three games' hand-rolled guards become. It runs
/// with no instance, no window and no frame — a `Store` with its
/// registrations run, a path on disk, and nothing else.
#[test]
fn a_shipped_document_that_holds_its_contract_is_not_refused() {
    let found = check("holds", HOLDS);
    assert!(found.is_empty(), "{found}");
}

/// §4.1 in as many words: a provider legitimately publishes intents no
/// shipped document raises, so that **reports** rather than refusing. This
/// is the half of `every_declared_raise_reaches_a_handler` that changes
/// grade in the migration, and P6 needs to know it did.
#[test]
fn an_intent_no_shipped_document_raises_reports() {
    let found = check("unraised", &HOLDS.replace("  withdraw(),\n", ""));
    assert!(!found.refuses(), "{found}");
    assert!(
        found
            .reports()
            .any(|f| matches!(f, Finding::Unraised { intent, .. } if intent == "withdraw")),
        "{found}"
    );
}

/// untold_lore's `every_declared_key_is_projected`, without the string
/// parsing: a selection naming a field no scope provides refuses, loud and
/// named.
#[test]
fn a_selection_naming_a_field_nothing_provides_refuses_and_names_it() {
    let found = check(
        "unprovided",
        &HOLDS.replace(
            "  status: string,",
            "  status: string,\n  launch_fade: float,",
        ),
    );
    assert!(found.refuses(), "{found}");
    let refusal = found
        .refusals()
        .find(|f| matches!(f, Finding::Unprovided { field, .. } if field == "launch_fade"))
        .expect("the field is named");
    assert!(refusal.to_string().contains("launch_fade"), "{refusal}");
}

/// A screen's own `state {}` block selects from **its node's scope**, on
/// top of the document's — which is what today's `"{id}::{field}"`
/// projection does by hand, and what the binding will do for real.
///
/// The mount below deliberately does not name the lobby's scope: only the
/// screen block reaches it, and only because the screen and the node share
/// a name. celia's arena `status` is the failing half — a screen selecting
/// a field the node it draws for never had.
#[test]
fn a_screens_own_selection_is_checked_against_its_nodes_scope() {
    let store = store();
    // The `events {}` block is document-wide and would need the lobby's
    // scope in the mount to validate; this test is about the read side.
    let no_raises = HOLDS.replace(
        "events {\n  pick(string),\n  confirm(),\n  withdraw(),\n};\n",
        "",
    );
    let root_only = |tag: &str, source: &str| {
        let path = shipped(tag, source);
        Documents::new(&store)
            .mounting(Mount::new(&path).selecting(FRONT).drawing(&["lobby"]))
            .check()
            .expect("the shipped document reads")
    };

    let found = root_only("screen-own-scope", &no_raises);
    assert!(
        !found.refuses(),
        "the screen reaches `roster` through its own node's scope: {found}"
    );

    let found = root_only(
        "screen-own-scope-missing",
        &no_raises.replace("can_confirm: bool }", "can_confirm: bool, stance: string }"),
    );
    assert!(
        found
            .refusals()
            .any(|f| matches!(f, Finding::Unprovided { field, .. } if field == "stance")),
        "neither the lobby's own scope nor the document's has a `stance`, which is \
         celia's arena `status` exactly — selected, provided by nothing, and silently \
         empty on screen today: {found}"
    );
}

/// celia's Back button, at `cargo test` time: a raise nothing accepts.
/// This is the wire-connectivity half of `every_button_reaches_a_route`
/// and the whole of `every_declared_raise_has_a_handler`.
#[test]
fn a_raise_nothing_accepts_refuses_and_names_the_intent() {
    let found = check(
        "unaccepted",
        &HOLDS.replace("  confirm(),", "  confirm(),\n  back(),"),
    );
    assert!(found.refuses(), "{found}");
    assert!(
        found
            .refusals()
            .any(|f| matches!(f, Finding::Unaccepted { intent, .. } if intent == "back")),
        "{found}"
    );
}

/// A raise the scope accepts, declared at the wrong shape. Positional,
/// because the block names no parameters — and the refusal still says
/// `character` rather than "argument 0", because the provider's name is
/// what the diagnostic keeps.
#[test]
fn a_raise_at_another_shape_refuses_and_names_the_parameter() {
    let found = check("shape", &HOLDS.replace("pick(string),", "pick(int),"));
    assert!(found.refuses(), "{found}");
    assert!(found.to_string().contains("character"), "{found}");
}

/// §4.1's unread direction, and celia's live example: a fact the host
/// computes every frame that nothing reads. It **reports** — a provider
/// legitimately publishes more than the shipped documents use — and it
/// names the scope and the field.
#[test]
fn a_field_provided_and_read_by_nothing_reports() {
    let found = check("unread", &HOLDS.replace("  status: string,\n", ""));
    assert!(!found.refuses(), "an unread field must not refuse: {found}");
    let report = found
        .reports()
        .find(|f| matches!(f, Finding::Unread { field, .. } if field == "status"))
        .expect("the unread field is named");
    assert!(report.to_string().contains("status"), "{report}");
}

/// Table-coverage drift, both ways, reporting rather than refusing: a
/// registered id nothing draws and a screen no node reaches.
#[test]
fn a_screen_and_a_route_id_that_disagree_report_without_refusing() {
    let found = check(
        "screens",
        &HOLDS.replace(r#"screen "lobby""#, r#"screen "muster""#),
    );
    assert!(!found.refuses(), "{found}");
    let printed = found.to_string();
    assert!(printed.contains("muster"), "{printed}");
    assert!(printed.contains("lobby"), "{printed}");
}

/// A document's `record` and a Rust struct meet as two lists of named
/// fields (§4.7). A renamed field in the document is a refusal naming the
/// dotted path down to it; the *type* names never take part.
#[test]
fn a_document_record_and_a_rust_struct_meet_structurally() {
    let found = check(
        "structural",
        &HOLDS
            .replace("record Tile {", "record RosterRow {")
            .replace("array<Tile>", "array<RosterRow>"),
    );
    assert!(
        !found.refuses(),
        "a renamed record is the same shape: {found}"
    );

    let found = check(
        "structural-drift",
        &HOLDS.replace("focused: bool };", "highlighted: bool };"),
    );
    assert!(found.refuses(), "{found}");
    let printed = found.to_string();
    assert!(printed.contains("roster[].highlighted"), "{printed}");
}

/// A document that will not read is its own error, not a finding: it has
/// real diagnostics waiting, and burying them under a contract complaint
/// is how the useful message gets lost.
#[test]
fn a_document_that_will_not_read_is_its_own_error() {
    let path = shipped("unreadable", "host_state { this is not a document");
    let failed = Documents::new(&store())
        .mounting(mount(&path))
        .check()
        .expect_err("the document does not parse");
    assert!(failed.to_string().contains("doc.ogh"), "{failed}");
}

// ── the hot-reload refusal ─────────────────────────────────────────────

/// A hot edit that breaks the contract is **rejected**, and the running
/// instance is not torn down (§4.1's last sentence).
///
/// Driven through the real file watcher, because the thing being proved is
/// about the moment of the swap: the edited document compiles perfectly —
/// it is a *contract* refusal, not a syntax error — and the check happens
/// before the candidate replaces anything. Afterwards the running document
/// is still the one that was mounted, still drawing, with its live host
/// state intact, and the file is still watched.
#[test]
fn a_refused_hot_edit_leaves_the_running_document_drawing() {
    let store = store();
    let path = shipped("hot-refuse", HOLDS);
    let mut chrome = Chrome::new(
        Ogham::watch(path.to_string_lossy().into_owned(), RuntimeConfig::new()).expect("watch"),
    );
    let mount = mount(&path);
    assert!(!chrome.check(&store, &mount).refuses(), "the mount holds");
    assert_eq!(chrome.refusal(), None);

    // A live value in the running runtime, so a torn-down instance would
    // be visible: a fresh runtime starts from the config's host state.
    chrome.project_root(
        "heading",
        ogham::runtime::value::Value::String("MUSTER".to_string()),
    );

    let drifted = HOLDS.replace(
        "  status: string,",
        "  status: string,\n  launch_fade: float,",
    );
    std::fs::write(&path, &drifted).expect("rewrite");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while chrome.refusal().is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "the watcher never delivered the edit"
        );
        chrome.frame_checked(&store, &mount, 640.0, 480.0, 1.0 / 60.0);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let why = chrome.refusal().expect("just checked");
    assert!(
        why.contains("launch_fade"),
        "the refusal names the field: {why}"
    );
    assert_eq!(
        chrome.error(),
        None,
        "the running document is not broken — the edit was refused, not the mount"
    );
    let live = chrome
        .ui_mut()
        .with_runtime_mut(|rt| rt.get_host_state("heading"));
    assert_eq!(
        live,
        Some(ogham::runtime::value::Value::String("MUSTER".to_string())),
        "the running instance still holds its live state, so it was never torn down"
    );
    assert!(
        !chrome.check(&store, &mount).refuses(),
        "the *running* document still holds its contract; the refused one never mounted"
    );
}

/// The other half: an edit that holds the contract still takes, and the
/// refusal that was standing is over. A gate that never opens is a gate
/// that gets removed.
#[test]
fn a_hot_edit_that_holds_the_contract_is_taken() {
    let store = store();
    let path = shipped("hot-accept", HOLDS);
    let mut chrome = Chrome::new(
        Ogham::watch(path.to_string_lossy().into_owned(), RuntimeConfig::new()).expect("watch"),
    );
    let mount = mount(&path);
    chrome.check(&store, &mount);

    // Drop the raise nothing uses: still legal, and observable in the
    // module schema afterwards.
    let edited = HOLDS.replace("  withdraw(),\n", "");
    std::fs::write(&path, &edited).expect("rewrite");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        chrome.frame_checked(&store, &mount, 640.0, 480.0, 1.0 / 60.0);
        let events = chrome
            .ui()
            .module_schema()
            .map(|s| s.event_names().len())
            .unwrap_or(0);
        if events == 2 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the watcher never delivered the edit"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(chrome.refusal(), None);
    assert_eq!(chrome.error(), None);
}
