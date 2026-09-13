//! Which file an import names — and, for a relative one, which directory it
//! is measured from.
//!
//! The rule is stated on `runtime::imports::ImportSpace`: a **relative**
//! path (`./`, `../`) names a file beside the document that wrote it; a
//! **named** one (prefixed, or bare) goes to the embedded map, then the
//! prefixes, then the project root.
//!
//! # What went wrong before
//!
//! The embedded map is one flat namespace keyed by the import string
//! exactly as written, and it used to be consulted first for every path.
//! So a host carrying an in-memory library under a key like
//! `"./widgets.ogh"` captured *every* document's `import "./widgets.ogh"`,
//! whatever directory that document sat in and whatever lay beside it on
//! disk. The document said "the file next to me" and got the host's.
//!
//! Nothing reported it, because there is nothing to report: the wrong file
//! parses, binds its names and draws. It surfaces only as a widget subtly
//! unlike the one being edited — and only if the two files differ visibly,
//! which two copies of one library often do not.
//!
//! It was live in a workspace where a game mounted an editor's `.ogh`
//! library as embedded sources under seven such keys, while the game's own
//! documents sat in a different crate's directory.
//!
//! # How these assert
//!
//! Each candidate file defines a function **only it** defines, and the
//! entry calls one of them. The right file being chosen is the document
//! executing at all; the wrong one is an unknown identifier. That is a
//! sharper instrument than comparing rendered text, and it needs no way to
//! read a global back out of the runtime.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::runtime::Runtime;

/// A scratch directory of our own, removed on drop. `ogham` carries no
/// dev-dependencies and this is not worth becoming the first one.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "ogham-import-{}-{}-{:?}",
            name,
            std::process::id(),
            std::thread::current().id(),
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, rel: &str, source: &str) -> PathBuf {
        let path = self.0.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("scratch subdir");
        }
        std::fs::write(&path, source).expect("write scratch file");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `let <name> = fn () { … };` — a marker only one candidate file carries.
fn only_defines(name: &str) -> String {
    format!("let {name} = fn () {{ Text {{ text: \"{name}\", style: {{}} }} }};")
}

fn renders(runtime: &mut Runtime) -> Result<(), String> {
    let module = runtime
        .get_module()
        .ok_or_else(|| "no entry module".to_string())?
        .clone();
    match runtime.execute_module(&module) {
        Ok(Value::Widget(_)) => Ok(()),
        Ok(other) => Err(format!("expected a Widget, got {other:?}")),
        Err(e) => Err(format!("{e:?}")),
    }
}

/// **The regression.** A document that came from a file, importing a
/// sibling by a relative path, gets the sibling — even when the host has an
/// embedded source registered under that exact spelling.
///
/// Before the fix the embedded source won, silently, and the file beside
/// the document was never read.
#[test]
fn a_relative_import_from_a_file_takes_the_file_beside_it() {
    let scratch = Scratch::new("sibling-wins");
    scratch.write("kit.ogh", &only_defines("from_the_sibling_on_disk"));
    let main = scratch.write(
        "main.ogh",
        "import \"./kit.ogh\";\nfrom_the_sibling_on_disk()",
    );

    // The trap: the same spelling, in memory, pointing somewhere else.
    let mut embedded = HashMap::new();
    embedded.insert(
        PathBuf::from("./kit.ogh"),
        only_defines("from_the_embedded_library"),
    );
    let config = RuntimeConfig::new().with_embedded_sources(embedded);

    let mut runtime = Runtime::from_file(&main, Some(config)).expect("from_file");
    renders(&mut runtime).expect(
        "a relative import in a file-backed document must name the file beside \
         it, never an embedded source of the same spelling",
    );
}

/// The other half, and the reason the fix is not "refuse relative keys in
/// the embedded map": a host with **no filesystem at all** carries its whole
/// library in memory and names those modules relatively. There is no
/// directory to be beside, so the flat lookup is the only answer there is.
///
/// This is `bytecode_imports.rs`'s arrangement, pinned here as a rule rather
/// than as a side effect.
#[test]
fn a_relative_import_with_no_file_behind_it_still_takes_the_embedded_source() {
    let mut embedded = HashMap::new();
    embedded.insert(
        PathBuf::from("./kit.ogh"),
        only_defines("from_the_embedded_library"),
    );
    let config = RuntimeConfig::new().with_embedded_sources(embedded);

    let entry = "import \"./kit.ogh\";\nfrom_the_embedded_library()";
    let mut runtime = Runtime::from_source(entry, Some(config)).expect("from_source");
    renders(&mut runtime).expect(
        "a root with no file behind it has no directory to resolve against, so \
         the embedded map is still what answers",
    );
}

/// And transitively: an embedded module's *own* relative imports stay in the
/// embedded map, however deep. This is the shape a shipped binary mounts —
/// an entry with no file, over a library of seven documents that import each
/// other by `./` — and a fix that consulted only the entry's origin would
/// break every hop below the first.
#[test]
fn an_embedded_module_resolves_its_own_relative_imports_in_the_map() {
    let mut embedded = HashMap::new();
    embedded.insert(
        PathBuf::from("./leaf.ogh"),
        only_defines("from_the_leaf"),
    );
    embedded.insert(
        PathBuf::from("./middle.ogh"),
        "import \"./leaf.ogh\";".to_string(),
    );
    let config = RuntimeConfig::new().with_embedded_sources(embedded);

    let entry = "import \"./middle.ogh\";\nfrom_the_leaf()";
    let mut runtime = Runtime::from_source(entry, Some(config)).expect("from_source");
    renders(&mut runtime).expect("the second hop must resolve in the map too");
}

/// A relative import is measured from the **importing document**, not from
/// the project root — so a module in a subdirectory importing a sibling gets
/// its own neighbour, not the root's file of the same name.
///
/// This was wrong before for the same reason: every relative path was joined
/// onto `project_root`. It went unnoticed only because every shipped
/// document in the workspace happened to sit in the root itself.
#[test]
fn a_relative_import_is_measured_from_the_importing_document() {
    let scratch = Scratch::new("nested");

    // Two files named alike, one in each directory. Only the nested one
    // defines what the entry ends up calling.
    scratch.write("kit.ogh", &only_defines("from_the_root_kit"));
    scratch.write("nested/kit.ogh", &only_defines("from_the_nested_kit"));
    scratch.write("nested/leaf.ogh", "import \"./kit.ogh\";");
    let main = scratch.write(
        "main.ogh",
        "import \"./nested/leaf.ogh\";\nfrom_the_nested_kit()",
    );

    let mut runtime = Runtime::from_file(&main, None).expect("from_file");
    renders(&mut runtime).expect(
        "a module one directory down must resolve its own sibling, not the \
         project root's file of the same name",
    );
}

/// A **named** import is not relative and keeps its old route: the embedded
/// map first, so a library carried in a binary answers to its own name
/// whatever directory the importing document sits in. This is what a `@lib`
/// style prefix rests on.
#[test]
fn a_named_import_still_reaches_the_embedded_library() {
    let scratch = Scratch::new("named");
    // A decoy beside the document, which a named import must NOT take.
    scratch.write("kit.ogh", &only_defines("from_the_decoy_sibling"));
    let main = scratch.write(
        "main.ogh",
        "import \"@lib/kit.ogh\";\nfrom_the_embedded_library()",
    );

    let mut embedded = HashMap::new();
    embedded.insert(
        PathBuf::from("@lib/kit.ogh"),
        only_defines("from_the_embedded_library"),
    );
    let config = RuntimeConfig::new().with_embedded_sources(embedded);

    let mut runtime = Runtime::from_file(&main, Some(config)).expect("from_file");
    renders(&mut runtime).expect("a prefixed import names a library, not a neighbour");
}

/// A bare import — no `./` — is a **named** one and goes to the project
/// root, which is what `examples/import/importer.ogh` has always relied on.
/// Pinned because the split is decided on the `./`, and a tidier-looking
/// reading ("anything without a prefix is relative") would break it.
#[test]
fn a_bare_import_is_named_and_not_relative() {
    let scratch = Scratch::new("bare");
    scratch.write("kit.ogh", &only_defines("from_the_project_root_kit"));
    let main = scratch.write("main.ogh", "import \"kit.ogh\";\nfrom_the_project_root_kit()");

    let mut runtime = Runtime::from_file(&main, None).expect("from_file");
    renders(&mut runtime).expect("a bare import resolves at the project root");
}

/// The prefix wins over the project root for a named path, and a source-tree
/// mount resolves it to a real file — the hot-reload arrangement, where a
/// library is authored on disk rather than embedded.
#[test]
fn a_prefix_maps_a_named_import_onto_a_directory() {
    let scratch = Scratch::new("prefix-dir");
    scratch.write("lib/kit.ogh", &only_defines("from_the_mapped_library"));
    // A decoy at the project root under the same leaf name.
    scratch.write("kit.ogh", &only_defines("from_the_root_decoy"));
    let main = scratch.write(
        "main.ogh",
        "import \"@lib/kit.ogh\";\nfrom_the_mapped_library()",
    );

    let config = RuntimeConfig::new().with_import_path("@lib", scratch.path().join("lib"));
    let mut runtime = Runtime::from_file(&main, Some(config)).expect("from_file");
    renders(&mut runtime).expect("a prefix names its own directory");
}
