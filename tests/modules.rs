//! WP-3.1: a document made of several files, and a hot reload that follows
//! the import graph (`docs/internal/APPLICATION_BUILD.md` Phase 3).
//!
//! The shapes here are the two target document splits Appendix A calls for
//! and neither game can write today: regency's `stationery.ogh`, imported
//! by the two instance documents its route table names, and celia's
//! `paperwork.ogh`, imported by three. Both split the same three things out
//! of one file — a palette, a record shape, and a helper that draws with
//! both — because those are what the split is *for*: a token edited once
//! has to reach every document that mounts it.
//!
//! That the *contract* also sees the whole graph — the harness reads a
//! split document, and the reload gate refuses an edit to a shared module
//! two files from the document it breaks — is `contract/tests/modules.rs`,
//! because asking it needs a store.

use std::path::{Path, PathBuf};

use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::Ogham;

// ── the shared module ──────────────────────────────────────────────────

/// The stationery: a palette token, the record shape both documents' host
/// state is declared at, and a helper that reads the token.
///
/// Every one of the three crosses the import in a different way — a `let`
/// through the environment, a `record` through the schema, a helper through
/// both — which is why one file carries all three.
const STATIONERY: &str = r##"
let ink = "#101010";
let gold = "#c8a24a";

record Card { title: string, weight: float };

let plate = fn (card: Card) {
  Text { text: card.title, style: { color: ink, size: 18 } }
};
"##;

/// A document that mounts the stationery. `main` reads the imported helper
/// and the imported token; `host_state {}` is declared at the imported
/// record shape, which is the half that did not resolve at all before
/// WP-3.1.
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
    let dir = std::env::temp_dir().join(format!("ogham-modules-{}-{tag}", std::process::id()));
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
fn rooted(dir: &Path) -> RuntimeConfig {
    RuntimeConfig::new().with_project_root(dir.to_path_buf())
}

fn mounted(path: &Path, dir: &Path) -> Ogham {
    Ogham::watch(path.to_string_lossy().into_owned(), rooted(dir)).expect("the document mounts")
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

/// The palette token as the mounted document sees it.
///
/// An imported module's top-level names land in host state when the import
/// executes, which is how a document reads `ink` as an ordinary identifier;
/// reading it back the same way is how a test sees the file two hops away.
fn token(ui: &Ogham) -> Option<String> {
    match ui.with_runtime_mut(|rt| rt.get_host_state("ink")) {
        Some(Value::String(s)) => Some(s),
        _ => None,
    }
}

// ── the split, as the two games need it ────────────────────────────────

/// regency's target split (Appendix A.2): one `stationery.ogh`, imported by
/// the two instance documents — `foyer.ogh` and `table.ogh`.
#[test]
fn regencys_stationery_is_imported_by_two_instance_documents() {
    let dir = scratch("regency");
    write(&dir, "stationery.ogh", STATIONERY);
    let foyer = write(&dir, "foyer.ogh", &document("the foyer"));
    let table = write(&dir, "table.ogh", &document("the table"));

    for path in [&foyer, &table] {
        let ui = mounted(path, &dir);
        let schema = ui
            .module_schema()
            .expect("the schema resolves against the import");
        assert!(
            schema.lookup_record("Card").is_some(),
            "`Card` crosses the import into the mounting document's schema"
        );
        assert_eq!(token(&ui).as_deref(), Some("#101010"));
    }
}

/// celia's target split (Appendix A.1): one `paperwork.ogh`, imported by
/// three documents — `menu.ogh`, `lobby.ogh` and `arena.ogh`.
#[test]
fn celias_paperwork_is_imported_by_three_documents() {
    let dir = scratch("celia");
    write(&dir, "stationery.ogh", STATIONERY);
    let documents: Vec<PathBuf> = ["menu.ogh", "lobby.ogh", "arena.ogh"]
        .iter()
        .map(|name| write(&dir, name, &document(name)))
        .collect();

    for path in &documents {
        assert_eq!(token(&mounted(path, &dir)).as_deref(), Some("#101010"));
    }
}

/// The property the split exists for: a palette token edited in the shared
/// module reaches **every** document mounting it, on save.
///
/// Both instances are watching their own file and neither file changed —
/// the edit is two files away from the thing that redraws. That is the
/// whole of "the watcher follows the import graph".
#[test]
fn a_palette_token_edited_in_the_shared_module_reaches_every_mounting_document() {
    let dir = scratch("propagate");
    let shared = write(&dir, "stationery.ogh", STATIONERY);
    let foyer = write(&dir, "foyer.ogh", &document("the foyer"));
    let table = write(&dir, "table.ogh", &document("the table"));

    let mut instances = [mounted(&foyer, &dir), mounted(&table, &dir)];
    for ui in &instances {
        assert_eq!(token(ui).as_deref(), Some("#101010"));
    }

    let edited = STATIONERY.replace("#101010", "#2b1d14");
    saving(&shared, &edited, "the edited palette token", || {
        for ui in &mut instances {
            ui.frame(640.0, 480.0, 1.0 / 60.0).expect("frame");
        }
        instances
            .iter()
            .all(|ui| token(ui).as_deref() == Some("#2b1d14"))
    });
}

/// An import *added* by a hot edit is watched from then on.
///
/// The watch set is a reading of the import graph, so it has to be re-read
/// whenever the graph can have changed. Before WP-3.1 it was built once at
/// mount and never again: a module a document acquired on Tuesday went
/// unwatched until the process restarted, and the author's next save did
/// nothing at all.
#[test]
fn a_module_a_hot_edit_adds_is_watched_from_then_on() {
    let dir = scratch("added");
    let shared = write(&dir, "stationery.ogh", STATIONERY);
    let path = write(
        &dir,
        "foyer.ogh",
        "let main = fn () { Text { text: \"no stationery yet\" } };\n",
    );
    let mut ui = mounted(&path, &dir);
    assert_eq!(token(&ui), None, "the document imports nothing yet");

    // The edit that acquires the module.
    let acquires = "import \"./stationery.ogh\";\nlet main = fn () { Text { text: ink } };\n";
    saving(&path, acquires, "the import that was added", || {
        ui.frame(640.0, 480.0, 1.0 / 60.0).expect("frame");
        token(&ui).as_deref() == Some("#101010")
    });

    // And now an edit to the newly-acquired module, which the watch set
    // did not know about when this instance was mounted.
    let edited = STATIONERY.replace("#101010", "#2b1d14");
    saving(
        &shared,
        &edited,
        "the edit to the module the document acquired",
        || {
            ui.frame(640.0, 480.0, 1.0 / 60.0).expect("frame");
            token(&ui).as_deref() == Some("#2b1d14")
        },
    );
}
