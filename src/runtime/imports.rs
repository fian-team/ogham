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
/// The three lookups `Runtime::execute_import` performs, in its order:
/// an embedded source keyed by the path exactly as written, then a prefix
/// mapping, then the project root. A missing `.ogh` extension is added.
#[derive(Clone, Debug, Default)]
pub struct ImportSpace {
    /// The directory a bare relative import resolves against.
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

    /// Where `path_str` points, and what is written there.
    ///
    /// `None` when the path does not resolve or cannot be read. Every
    /// caller here is best-effort by design: the real import reports the
    /// real error at execution time, with its own diagnostics, and a
    /// second complaint from a pre-scan would bury it.
    pub fn resolve(&self, path_str: &str) -> Option<Resolved> {
        if let Some(source) = self.embedded.get(Path::new(path_str)) {
            return Some(Resolved {
                key: PathBuf::from(path_str),
                file: None,
                source: source.clone(),
            });
        }
        let mut resolved = None;
        for (prefix, base) in &self.import_paths {
            if let Some(rest) = path_str.strip_prefix(prefix.as_str()) {
                let rest = rest.strip_prefix('/').unwrap_or(rest);
                resolved = Some(base.join(rest));
                break;
            }
        }
        let mut resolved = match resolved {
            Some(path) => path,
            None => self.project_root.as_ref()?.join(path_str),
        };
        if resolved.extension().is_none() {
            resolved.set_extension("ogh");
        }
        let source = std::fs::read_to_string(&resolved).ok()?;
        let key = resolved.canonicalize().unwrap_or_else(|_| resolved.clone());
        Some(Resolved {
            key,
            file: Some(resolved),
            source,
        })
    }
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
pub fn walk(module: &Function, space: &ImportSpace) -> Crossing {
    let mut found = Crossing::default();
    let mut seen = HashSet::new();
    walk_into(module, space, &mut seen, &mut found);
    found
}

fn walk_into(
    module: &Function,
    space: &ImportSpace,
    seen: &mut HashSet<PathBuf>,
    found: &mut Crossing,
) {
    for statement in &module.body.statement_list {
        let Statement::Import(import) = statement else {
            continue;
        };
        let Some(resolved) = space.resolve(import.get_path()) else {
            continue;
        };
        if !seen.insert(resolved.key.clone()) {
            continue;
        }
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
        walk_into(&imported, space, seen, found);
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

        let found = walk(&module, &ImportSpace::rooted_at(&dir));

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

        let found = walk(&module, &ImportSpace::rooted_at(&dir));

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

        let found = walk(&module, &ImportSpace::rooted_at(&dir));

        assert_eq!(found.files.len(), 2, "{:?}", found.files);
        assert!(found.values.contains("a") && found.values.contains("b"));
    }

    #[test]
    fn an_import_that_does_not_resolve_contributes_nothing() {
        let dir = scratch("missing");
        let module = parse("import \"./nowhere.ogh\";\nlet main = fn () { 1 };");
        let found = walk(&module, &ImportSpace::rooted_at(&dir));
        assert!(found.values.is_empty());
        assert!(found.files.is_empty());
    }
}
