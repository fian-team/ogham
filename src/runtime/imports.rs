//! The import graph, walked once and read by everyone who needs to know
//! what crosses it.
//!
//! A document that imports is a document made of several files, and three
//! separate parts of this crate have to agree about which files those are:
//! the **compiler**, which resolves a strict-mode identifier against the
//! names an import provides; the **schema**, which resolves a `record`
//! reference against the records an import provides; and the **watcher**,
//! which reloads the document when any of them is saved. Before
//! `APPLICATION_BUILD.md` WP-3.1 those three answered separately and
//! disagreed: the compiler pre-scanned direct imports for `let` names only,
//! the schema was handed an empty import map by every caller in the crate,
//! and the watcher was built once at mount and never rebuilt. A document
//! split across files — which is the whole of WP-3.1 — needs one answer.
//!
//! So: [`ImportSpace`] says where an import path resolves from, and
//! [`Crossing`] is what a walk of the graph found. Both are read by the
//! runtime (which compiles), by [`crate::runtime::schema::load_schema_in`]
//! (which does not), and by the reload gate.
//!
//! # The walk mirrors execution, deliberately
//!
//! `Runtime::execute_import` runs an imported module *in the importing
//! runtime*, so a module imported by a module imported by the document has
//! already copied its top-level names into the shared environment by the
//! time the document's own body runs. That is why this walk is transitive:
//! a name the runtime will resolve and a name the compiler will accept have
//! to be the same set, or a helper two files away compiles as an unknown
//! identifier and runs perfectly.
//!
//! The one asymmetry is narrowing, and it is execution's: `import { a } from
//! "x.ogh"` narrows *`x`'s own* declarations to `a`, and whatever `x`
//! imported arrives beside it unnarrowed, because `x`'s imports were copied
//! into the shared environment before the narrowing happened. This walk
//! does the same thing rather than the tidier thing, because the tidier
//! thing would be a second answer.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::parser::{Function, Parser, Statement};
use crate::runtime::schema::{RecordSchema, SelectionSchema};
use crate::scanner::Scanner;

/// Where an import path resolves from.
///
/// Two kinds of import path, and they are answered differently:
///
/// - A **named** path — `"@lib/widgets.ogh"`, `"widgets.ogh"` — names a
///   library or a document at the project root. Three lookups in this
///   order: an embedded source keyed by the path exactly as written, then
///   a prefix mapping, then the project root.
/// - A **relative** path — `"./widgets.ogh"`, `"../kit/widgets.ogh"` —
///   names a file *next to the document that wrote it*, and resolves
///   against that document's own directory. Nothing else is consulted.
///
/// # Why relative is not just another name
///
/// The embedded map is one flat namespace with no notion of where the
/// importing document lives, and it used to be consulted first for every
/// path, relative ones included. So a host that carried an in-memory
/// library under a key like `"./widgets.ogh"` captured **every** document's
/// `import "./widgets.ogh"`, whatever directory that document was in and
/// whatever sat next to it on disk. The import said "the file beside me"
/// and meant "whatever the host happened to register under that spelling".
/// Nothing reported it: the wrong file parses, binds its names, and draws.
///
/// That was live in this workspace. A game mounted an editor's `.ogh`
/// library as embedded sources keyed `"./widgets.ogh"`, `"./theme.ogh"`
/// and five more; the game's own documents sat in a different crate's
/// directory. The day either side grew a file of a name the other had
/// registered, every relative import of it would have silently crossed the
/// crate boundary. A named prefix per library is the tidy way to write
/// this and remains the advice — but the advice was load-bearing, which is
/// what made it a defect rather than a style.
///
/// A missing `.ogh` extension is added.
///
/// Relative resolution needs the importing document, which every caller
/// knows: [`walk_into`] carries the parent it descended from, and
/// `Runtime::execute_import` carries the document whose statement it is
/// running. `None` means the importer has no file behind it — an embedded
/// module, or a root built by `Runtime::from_source` — and for those a
/// relative path falls back to the flat lookup, because "beside me" cannot
/// mean anything else when there is no directory to be beside. That is
/// what keeps a host with no filesystem at all working.
#[derive(Clone, Debug, Default)]
pub struct ImportSpace {
    /// The directory a **named** import resolves against, after the
    /// embedded map and the prefixes have both declined it.
    pub project_root: Option<PathBuf>,
    /// Prefix → base directory, for imports written against a named
    /// library rather than against the project root.
    pub import_paths: HashMap<String, PathBuf>,
    /// Sources with no file behind them, keyed by the import path string
    /// exactly as written.
    pub embedded: HashMap<PathBuf, String>,
}

/// One resolved import: the key it is cached and watched under, and its
/// source text.
pub struct Resolved {
    /// The canonical path, or the import string itself for an embedded
    /// source. Also the cycle key.
    pub key: PathBuf,
    /// `None` for an embedded source, which no watcher can watch.
    pub file: Option<PathBuf>,
    pub source: String,
}

impl ImportSpace {
    /// An import space rooted at one directory and nothing else — what a
    /// standalone schema load uses when no host has configured one.
    pub fn rooted_at(root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: Some(root.into()),
            ..Self::default()
        }
    }

    /// Where `path_str`, written in the document at `from`, points — and
    /// what is written there.
    ///
    /// `from` is the importing document's own file, or `None` where it has
    /// none (an embedded module, or a root built from source). See the
    /// type's own documentation for what that changes.
    ///
    /// `None` when the path does not resolve or cannot be read. Every
    /// caller here is best-effort by design: the real import reports the
    /// real error at execution time, with its own diagnostics, and a
    /// second complaint from a pre-scan would bury it.
    pub fn resolve(&self, path_str: &str, from: Option<&Path>) -> Option<Resolved> {
        match self.locate(path_str, from)? {
            Located::Embedded(source) => Some(Resolved {
                key: PathBuf::from(path_str),
                file: None,
                source,
            }),
            Located::File(path) => {
                let source = std::fs::read_to_string(&path).ok()?;
                let key = path.canonicalize().unwrap_or_else(|_| path.clone());
                Some(Resolved {
                    key,
                    file: Some(path),
                    source,
                })
            }
        }
    }

    /// The resolution rule itself, without reading anything.
    ///
    /// **This is the one answer.** It was written twice — once here for the
    /// walkers that read the import graph without running it, once inside
    /// `Runtime::execute_import` for the run itself — and two answers to
    /// one question is how a pre-scan comes to disagree with execution
    /// about which file a name came from. Both call this now.
    pub(crate) fn locate(&self, path_str: &str, from: Option<&Path>) -> Option<Located> {
        // A relative path written in a document that has a file means the
        // directory that document is in, and nothing else. No embedded
        // lookup, no prefix, no project root: "beside me" has exactly one
        // answer, and consulting a flat map for it is what let one crate's
        // library capture another crate's siblings.
        if is_relative(path_str) {
            if let Some(dir) = from.and_then(Path::parent) {
                return Some(Located::File(with_ogh(dir.join(path_str))));
            }
        }

        if let Some(source) = self.embedded.get(Path::new(path_str)) {
            return Some(Located::Embedded(source.clone()));
        }

        for (prefix, base) in &self.import_paths {
            if let Some(rest) = path_str.strip_prefix(prefix.as_str()) {
                let rest = rest.strip_prefix('/').unwrap_or(rest);
                return Some(Located::File(with_ogh(base.join(rest))));
            }
        }

        Some(Located::File(with_ogh(
            self.project_root.as_ref()?.join(path_str),
        )))
    }
}

/// Where a path landed, before anything has been read.
pub(crate) enum Located {
    Embedded(String),
    File(PathBuf),
}

/// A path that names a file beside the document that wrote it.
///
/// Only `./` and `../` — a bare `"widgets.ogh"` is a **named** import and
/// goes to the library and the project root, which is what it has always
/// meant here and what `examples/import/importer.ogh` relies on.
fn is_relative(path_str: &str) -> bool {
    path_str.starts_with("./") || path_str.starts_with("../")
}

fn with_ogh(mut path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.set_extension("ogh");
    }
    path
}

/// What a walk of one module's import graph found.
///
/// Everything here is what the importing module *gains*: the names it may
/// reference, the record shapes those names may be declared at, and the
/// files the whole thing is made of.
#[derive(Clone, Debug, Default)]
pub struct Crossing {
    /// Top-level `let` names, so strict-mode identifier resolution accepts
    /// a helper that lives in another file.
    pub values: BTreeSet<String>,
    /// Records, keyed by the name they are known by, so a `host_state` or
    /// a `record` field may be declared at a shape another file owns.
    pub records: BTreeMap<String, RecordSchema>,
    /// `select` blocks, in the order the walk met them — §4.7's
    /// fragments. A shared module states its selection once and it
    /// travels with every document that mounts it, to be validated
    /// against that mount's scopes.
    ///
    /// Unnarrowed by a named import, unlike the two above: a `select`
    /// block has no name to import, and the helper that *was* imported
    /// reads the fields it names. Narrowing here would compile a
    /// fragment whose fields nothing had checked.
    pub selections: Vec<SelectionSchema>,
    /// Every file the graph reached, in discovery order — what a watcher
    /// watches. Embedded sources are absent: they have no file behind
    /// them, and a watcher handed one would refuse to start.
    pub files: Vec<PathBuf>,
    /// The dotted reads every imported file makes off a top-level name
    /// ([`crate::runtime::reads`]).
    ///
    /// Here for the same reason `selections` is: a fragment binds its
    /// names in its own file *and* in the mounting document, so the
    /// helper family that actually reads `hud.clock` is often two files
    /// away from the `select` that named `hud`. A check that saw only the
    /// mounting document would hold the guarantee exactly where nobody
    /// keeps their helpers.
    pub reads: Vec<String>,
}

/// Walk `module`'s imports, transitively, and collect what crosses.
///
/// `at` is the file `module` was read from, or `None` for one with none.
/// It is what a relative import in `module` resolves against — see
/// [`ImportSpace`].
pub fn walk(module: &Function, space: &ImportSpace, at: Option<&Path>) -> Crossing {
    let mut found = Crossing::default();
    let mut seen = HashSet::new();
    walk_into(module, space, at, &mut seen, &mut found);
    found
}

fn walk_into(
    module: &Function,
    space: &ImportSpace,
    at: Option<&Path>,
    seen: &mut HashSet<PathBuf>,
    found: &mut Crossing,
) {
    for statement in &module.body.statement_list {
        let Statement::Import(import) = statement else {
            continue;
        };
        let Some(resolved) = space.resolve(import.get_path(), at) else {
            continue;
        };
        if !seen.insert(resolved.key.clone()) {
            continue;
        }
        // Kept before the move into `found.files`: it is what the
        // imported module's OWN relative imports resolve against, one
        // level down.
        let imported_at = resolved.file.clone();
        if let Some(file) = resolved.file {
            found.files.push(file);
        }
        let tokens = Scanner::new(resolved.source).scan();
        let Ok(imported) = Parser::new(tokens).parse() else {
            continue;
        };
        // Depth first, so a name the imported module re-exports by
        // importing it is already in `found` when the narrowing below
        // runs — and unnarrowed, which is what execution does.
        walk_into(&imported, space, imported_at.as_deref(), seen, found);
        // Unnarrowed, like the selections: a narrowed import still
        // *executes* the whole file, so every read in it is a read the
        // mounted document makes.
        found.reads.extend(crate::runtime::reads::of(&imported));
        let wanted = import.get_names().clone();
        let takes = |name: &str| match &wanted {
            Some(names) => names.iter().any(|n| n == name),
            None => true,
        };
        for statement in &imported.body.statement_list {
            match statement {
                Statement::Declare(declare) => {
                    let name = declare.get_identifier().get();
                    if takes(&name) {
                        found.values.insert(name);
                    }
                }
                Statement::RecordDeclaration(record) => {
                    if !takes(&record.name) {
                        continue;
                    }
                    if let Ok(schema) = crate::runtime::schema::record_schema_of(record) {
                        found.records.insert(record.name.clone(), schema);
                    }
                }
                Statement::SelectDeclaration(select) => {
                    found.selections.push(SelectionSchema {
                        scope: select.scope.clone(),
                        fields: select.fields.iter().map(|f| f.name.clone()).collect(),
                        decl_span: Some(select.span),
                        imported: true,
                    });
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ogham-imports-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn parse(source: &str) -> Function {
        Parser::new(Scanner::new(source.to_string()).scan())
            .parse()
            .expect("parse")
    }

    #[test]
    fn a_walk_reaches_a_module_two_imports_away() {
        let dir = scratch("transitive");
        std::fs::write(dir.join("palette.ogh"), "let ink = \"#101010\";\n").expect("write");
        std::fs::write(
            dir.join("stationery.ogh"),
            "import \"./palette.ogh\";\nrecord Card { title: string };\nlet rule = fn () { ink };\n",
        )
        .expect("write");
        let module = parse("import \"./stationery.ogh\";\nlet main = fn () { rule() };\n");

        let found = walk(&module, &ImportSpace::rooted_at(&dir), None);

        assert!(found.values.contains("rule"), "{:?}", found.values);
        assert!(
            found.values.contains("ink"),
            "a name two files away resolves at run time, so it must resolve at compile time: {:?}",
            found.values
        );
        assert!(found.records.contains_key("Card"), "{:?}", found.records);
        assert_eq!(found.files.len(), 2, "{:?}", found.files);
    }

    #[test]
    fn a_named_import_narrows_the_module_it_names_and_nothing_below_it() {
        let dir = scratch("narrowing");
        std::fs::write(dir.join("palette.ogh"), "let ink = \"#101010\";\n").expect("write");
        std::fs::write(
            dir.join("stationery.ogh"),
            "import \"./palette.ogh\";\nlet rule = fn () { ink };\nlet seal = fn () { ink };\n",
        )
        .expect("write");
        let module =
            parse("import { rule } from \"./stationery.ogh\";\nlet main = fn () { rule() };");

        let found = walk(&module, &ImportSpace::rooted_at(&dir), None);

        assert!(found.values.contains("rule"));
        assert!(!found.values.contains("seal"), "the import named one");
        assert!(
            found.values.contains("ink"),
            "what `stationery` imported arrives beside it, because that is what \
             `execute_import` does: {:?}",
            found.values
        );
    }

    #[test]
    fn a_cycle_is_walked_once() {
        let dir = scratch("cycle");
        std::fs::write(dir.join("a.ogh"), "import \"./b.ogh\";\nlet a = 1;\n").expect("write");
        std::fs::write(dir.join("b.ogh"), "import \"./a.ogh\";\nlet b = 2;\n").expect("write");
        let module = parse("import \"./a.ogh\";\nlet main = fn () { a };");

        let found = walk(&module, &ImportSpace::rooted_at(&dir), None);

        assert_eq!(found.files.len(), 2, "{:?}", found.files);
        assert!(found.values.contains("a") && found.values.contains("b"));
    }

    #[test]
    fn an_import_that_does_not_resolve_contributes_nothing() {
        let dir = scratch("missing");
        let module = parse("import \"./nowhere.ogh\";\nlet main = fn () { 1 };");
        let found = walk(&module, &ImportSpace::rooted_at(&dir), None);
        assert!(found.values.is_empty());
        assert!(found.files.is_empty());
    }
}
