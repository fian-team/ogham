//! WP-3.1 meets WP-2.4: the contract sees a document made of several
//! files (`../docs/internal/APPLICATION_BUILD.md` Phase 3, §4.1).
//!
//! The import graph itself — which files a document is made of, and which
//! of them a hot reload watches — is `ogham/tests/modules.rs`, which needs
//! no store. What is here is the other half: the harness reads a split
//! document whole, and the reload gate refuses an edit to a shared module
//! two files away from the document it breaks.
//!
//! The fixture is regency's target split (Appendix A.2): one
//! `stationery.ogh` holding a palette token, a record shape and a helper,
//! imported by the instance document that mounts it.

use std::path::{Path, PathBuf};

use contract::{Checked, Documents, Mount, Scope, Store};
use ogham::route::Chrome;
use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::Ogham;

use structure::schema::{Field, Kind, Lit, Schema};

/// The stationery: a palette token, the record shape the document's host
/// state is declared at, and a helper that reads the token.
const STATIONERY: &str = r##"
let ink = "#101010";
let gold = "#c8a24a";

record Card { title: string, weight: float };

let plate = fn (card: Card) {
  Text { text: card.title, style: { color: ink, size: 18 } }
};
"##;

/// A document that mounts the stationery, with its `host_state {}`
/// declared at the imported record shape.
fn document(title: &str) -> String {
    format!(
        r#"
import "./stationery.ogh";

host_state {{
  card: Card,
}};

let main = fn () {{
  Flex {{
    style: {{ background_color: gold }},
    children: [ plate(card), Text {{ text: "{title}", style: {{ color: ink }} }} ],
  }}
}};
"#
    )
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("contract-modules-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn write(dir: &Path, name: &str, source: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, source).expect("write");
    path
}

/// The config a host builds for a document with imports: a project root, so
/// `./stationery.ogh` resolves the same way at run time as it does in the
/// schema.
fn mounted(path: &Path, dir: &Path) -> Ogham {
    let config = RuntimeConfig::new().with_project_root(dir.to_path_buf());
    Ogham::watch(path.to_string_lossy().into_owned(), config).expect("the document mounts")
}

/// Save an edit, then drive frames until `settled` answers — or fail
/// naming what never happened.
///
/// The save repeats while the wait runs, and that is not belt-and-braces:
/// a watcher registers its directory *asynchronously*, so an edit written
/// in the same instant an instance was mounted can be missed outright, and
/// the test would then hang on an event that is never coming. Re-saving
/// turns that into a slower pass rather than a flake nobody can reproduce.
fn saving(path: &Path, source: &str, what: &str, mut settled: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut next_save = std::time::Instant::now();
    while !settled() {
        assert!(
            std::time::Instant::now() < deadline,
            "the watcher never delivered: {what}"
        );
        if std::time::Instant::now() >= next_save {
            std::fs::write(path, source).expect("rewrite");
            next_save = std::time::Instant::now() + std::time::Duration::from_secs(1);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// The scope the split documents select against.
#[derive(Clone, Debug, Default, PartialEq)]
struct Table {
    card: Card,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Card {
    title: String,
    weight: f32,
}

impl Schema for Card {
    fn reflect() -> Kind {
        Kind::Record(vec![
            Field::new("title", Kind::Str),
            Field::new("weight", Kind::Float),
        ])
    }
    fn at_mount(_: Option<&Lit>) -> Self {
        Self::default()
    }
    fn type_name() -> Option<&'static str> {
        Some("Card")
    }
}

impl Schema for Table {
    fn reflect() -> Kind {
        Kind::Record(vec![Field::new("card", Card::reflect())])
    }
    fn at_mount(_: Option<&Lit>) -> Self {
        Self::default()
    }
    fn type_name() -> Option<&'static str> {
        Some("Table")
    }
}

const TABLE: Scope = Scope::Node("table");

fn store() -> Store {
    let mut store = Store::new();
    store.provides::<Table>(TABLE).expect("the table's scope");
    store
}

/// The `cargo test` harness reads a split document whole.
///
/// A `host_state` field declared at a record another file owns used to make
/// the harness answer [`Unreadable`](contract::Unreadable) — "unknown record
/// `Card`" — so a repo that split its documents lost the contract check
/// that the split was supposed to survive.
#[test]
fn the_contract_harness_reads_a_document_split_across_files() {
    let dir = scratch("harness");
    write(&dir, "stationery.ogh", STATIONERY);
    let table = write(&dir, "table.ogh", &document("the table"));

    let found = Documents::new(&store())
        .mounting(Mount::new(&table).selecting(TABLE))
        .check()
        .expect("a split document reads");
    assert!(!found.refuses(), "{found}");
}

/// A transitive reload goes through WP-2.4's gate, not around it.
///
/// The edit is to the **shared** module, and it breaks the contract of a
/// document two files away: the record `Card` loses the field the table's
/// scope provides, so every document declared at that shape now selects a
/// shape nothing provides. The gate refuses the candidate and names the
/// path, and the running instance goes on drawing with its live state.
#[test]
fn an_edit_to_a_shared_module_is_refused_by_the_gate_that_the_document_would_fail() {
    let dir = scratch("gate");
    let shared = write(&dir, "stationery.ogh", STATIONERY);
    let table = write(&dir, "table.ogh", &document("the table"));

    let store = store();
    let mount = Mount::new(&table).selecting(TABLE);
    let mut chrome = Chrome::new(mounted(&table, &dir));
    assert!(!chrome.check(&store, &mount).refuses(), "the mount holds");

    chrome.project_root("__witness", Value::String("standing".to_string()));

    let drifted = STATIONERY.replace(
        "record Card { title: string, weight: float };",
        "record Card { title: string, heft: float };",
    );
    saving(&shared, &drifted, "the refused edit", || {
        chrome.frame_checked(&store, &mount, 640.0, 480.0, 1.0 / 60.0);
        chrome.refusal().is_some()
    });

    let why = chrome.refusal().expect("just checked");
    assert!(
        why.contains("card.heft") || why.contains("card.weight"),
        "the refusal names the path down to where the shapes stopped agreeing: {why}"
    );
    assert_eq!(chrome.error(), None, "the edit was refused, not the mount");
    assert_eq!(
        chrome
            .ui_mut()
            .with_runtime_mut(|rt| rt.get_host_state("__witness")),
        Some(Value::String("standing".to_string())),
        "the running instance kept its live state, so it was never torn down"
    );

    // And the same gate opens again when the shared module is put right.
    saving(&shared, STATIONERY, "the healed edit", || {
        chrome.frame_checked(&store, &mount, 640.0, 480.0, 1.0 / 60.0);
        chrome.refusal().is_none()
    });
}
