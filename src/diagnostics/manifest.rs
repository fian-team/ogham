//! On-disk JSON shape of a binding manifest.
//!
//! A manifest captures the schema of one Rust-side derive (an
//! `OghamState` struct or an `OghamMsg` enum) at proc-macro
//! expansion time, paired with a back-pointer to the `.ogh` module
//! it binds to. The diagnostic backend reads these manifests and
//! checks them against parsed `.ogh` modules to detect cross-side
//! drift before runtime.
//!
//! ## File layout (one manifest per binding)
//!
//! Manifests live at
//! `<CARGO_TARGET_DIR>/ogham/<binding-id>.json` (or
//! `<CARGO_MANIFEST_DIR>/target/ogham/<binding-id>.json` when
//! `CARGO_TARGET_DIR` isn't set). The `<binding-id>` is a
//! sanitized concatenation of `kind`, the binding-module path, and
//! the Rust type name; the body carries the fully-qualified Rust
//! path so cross-crate collisions are diagnosable rather than
//! silent.
//!
//! ## Two kinds, one tagged union
//!
//! [`Manifest::State`] holds the `host_state {}` shape derived from
//! an `OghamState` struct. [`Manifest::Events`] holds the events
//! signature map derived from an `OghamMsg` enum. The on-wire JSON
//! discriminates via a top-level `"kind": "state" | "events"` field
//! (serde tagged-union representation), so the consuming side picks
//! the right branch from one read.
//!
//! ## TypeRef encoding
//!
//! Field types and event arg types travel as canonical-string
//! [`crate::parser::typed_bindings::TypeRef`] forms (e.g. `int`,
//! `array<Item>`, `map<string, Player>`, `int?`). The macro side
//! hand-emits these strings without depending on serde; the reader
//! parses them back via
//! [`crate::parser::typed_bindings::TypeRef::from_canonical_string`].
//! The round-trip property is tested in the parser module.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::runtime::schema::{OghamMsg, OghamState};

/// One binding manifest. The on-wire JSON discriminates the
/// variant via a `"kind"` field at the same level as the rest
/// of the body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Manifest {
    /// An `OghamState` derive's `host_state {}` shape.
    State(StateManifest),
    /// An `OghamMsg` derive's events signature map.
    Events(EventsManifest),
}

/// Manifest for an `OghamState` derive — captures the host-state
/// shape the Rust struct expects.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StateManifest {
    /// Fully-qualified Rust path of the bound type, e.g.
    /// `untold_lore::ui::chest::ChestUiState`. Used as the
    /// `binding_id` when emitting per-binding diagnostics.
    pub binding: String,
    /// Path of the `.ogh` module this binding pairs with, relative
    /// to the consumer crate's `CARGO_MANIFEST_DIR`. The diagnostic
    /// backend resolves this to an absolute path when matching
    /// against an open `.ogh` file.
    pub ogh_module: String,
    /// Source location of the derive, for `related_information` in
    /// emitted diagnostics.
    pub rust_source: RustSourceLoc,
    /// The `host_state {}` shape derived from the struct's fields.
    pub host_state: ManifestRecord,
}

/// Manifest for an `OghamMsg` derive — captures the events
/// signature map the Rust enum produces.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventsManifest {
    pub binding: String,
    pub ogh_module: String,
    pub rust_source: RustSourceLoc,
    /// Event name → argument-type list. Keyed by the `.ogh`-side
    /// event name (snake_case by convention; overridable via
    /// `#[ogham(rename = "...")]`).
    pub events: BTreeMap<String, ManifestEvent>,
}

/// A record-shaped schema (used for `host_state`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ManifestRecord {
    /// Field name → field schema. `BTreeMap` for deterministic
    /// JSON output.
    pub fields: BTreeMap<String, ManifestField>,
}

/// One field in a record-shaped schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestField {
    /// `TypeRef` in canonical-string form; e.g. `array<Item>`,
    /// `map<string, int>`, `int?`. Matches the exact surface
    /// syntax used in `.ogh` source.
    pub ty: String,
}

/// One event in an events-shaped schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestEvent {
    /// Argument types in declaration order, each in canonical-string
    /// `TypeRef` form.
    pub args: Vec<String>,
}

/// Source location of the Rust derive that produced this manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RustSourceLoc {
    /// Path of the source file, as captured at proc-macro expansion
    /// time. Stored verbatim — may be absolute (when cargo passes a
    /// rooted path) or workspace-relative (when it doesn't).
    pub file: String,
    pub line: u32,
    pub column: u32,
}

impl StateManifest {
    /// Synthesize a manifest from a generic `S: OghamState`. Used by
    /// the runtime wrapper around `check_schemas_match` (see
    /// `src/typed.rs`) so the runtime path runs against the same
    /// backend as the static path. `RustSourceLoc` is empty —
    /// matches the on-disk shape that P0-M3 emits on stable Rust.
    pub fn from_state<S: OghamState>(ogh_module: &str) -> Self {
        let schema = S::ogham_record_schema();
        let mut fields = BTreeMap::new();
        for (name, fs) in &schema.fields {
            fields.insert(
                name.clone(),
                ManifestField {
                    ty: fs.ty.to_canonical_string(),
                },
            );
        }
        StateManifest {
            binding: std::any::type_name::<S>().to_string(),
            ogh_module: ogh_module.to_string(),
            rust_source: RustSourceLoc {
                file: String::new(),
                line: 0,
                column: 0,
            },
            host_state: ManifestRecord { fields },
        }
    }
}

impl EventsManifest {
    /// Synthesize an events manifest from a generic `M: OghamMsg`.
    /// Pair to `StateManifest::from_state`; same caveats apply.
    pub fn from_events<M: OghamMsg>(ogh_module: &str) -> Self {
        let derived = M::ogham_events();
        let mut events = BTreeMap::new();
        for (name, sig) in &derived {
            events.insert(
                name.clone(),
                ManifestEvent {
                    args: sig.args.iter().map(|t| t.to_canonical_string()).collect(),
                },
            );
        }
        EventsManifest {
            binding: std::any::type_name::<M>().to_string(),
            ogh_module: ogh_module.to_string(),
            rust_source: RustSourceLoc {
                file: String::new(),
                line: 0,
                column: 0,
            },
            events,
        }
    }
}

impl Manifest {
    /// The `.ogh` module this binding targets, regardless of variant.
    pub fn ogh_module(&self) -> &str {
        match self {
            Self::State(s) => &s.ogh_module,
            Self::Events(e) => &e.ogh_module,
        }
    }

    /// The fully-qualified Rust binding path, regardless of variant.
    pub fn binding(&self) -> &str {
        match self {
            Self::State(s) => &s.binding,
            Self::Events(e) => &e.binding,
        }
    }

    /// Read a manifest from disk. Returns the JSON-parse error
    /// boxed inside `io::Error::InvalidData` so callers can match
    /// on `io::ErrorKind` uniformly.
    pub fn read(path: &Path) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Write a manifest to disk atomically: serialize, write to a
    /// `<path>.tmp` sibling, then rename onto `path`. The parent
    /// directory is created if it doesn't exist. The
    /// tempfile-rename is what keeps a concurrent reader (the LSP
    /// file watcher in P1) from observing a half-written file.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut tmp_path = path.as_os_str().to_os_string();
        tmp_path.push(".tmp");
        let tmp_path = std::path::PathBuf::from(tmp_path);
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state() -> StateManifest {
        let mut fields = BTreeMap::new();
        fields.insert("selected".into(), ManifestField { ty: "int".into() });
        fields.insert(
            "items".into(),
            ManifestField {
                ty: "array<Item>".into(),
            },
        );
        StateManifest {
            binding: "untold_lore::ui::chest::ChestUiState".into(),
            ogh_module: "data/ui/chest.ogh".into(),
            rust_source: RustSourceLoc {
                file: "src/ui/chest_ui.rs".into(),
                line: 42,
                column: 10,
            },
            host_state: ManifestRecord { fields },
        }
    }

    fn sample_events() -> EventsManifest {
        let mut events = BTreeMap::new();
        events.insert("open_chest".into(), ManifestEvent { args: vec![] });
        events.insert(
            "take_item".into(),
            ManifestEvent {
                args: vec!["int".into()],
            },
        );
        events.insert(
            "transfer".into(),
            ManifestEvent {
                args: vec!["int".into(), "Item".into()],
            },
        );
        EventsManifest {
            binding: "untold_lore::ui::chest::ChestUiMsg".into(),
            ogh_module: "data/ui/chest.ogh".into(),
            rust_source: RustSourceLoc {
                file: "src/ui/chest_ui.rs".into(),
                line: 73,
                column: 10,
            },
            events,
        }
    }

    #[test]
    fn state_manifest_round_trips_through_json() {
        let original = Manifest::State(sample_state());
        let json = serde_json::to_string(&original).unwrap();
        assert!(json.contains("\"kind\":\"state\""));
        let parsed: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn events_manifest_round_trips_through_json() {
        let original = Manifest::Events(sample_events());
        let json = serde_json::to_string(&original).unwrap();
        assert!(json.contains("\"kind\":\"events\""));
        let parsed: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn empty_record_round_trips() {
        let m = Manifest::State(StateManifest {
            binding: "x::Y".into(),
            ogh_module: "a.ogh".into(),
            rust_source: RustSourceLoc {
                file: "x.rs".into(),
                line: 1,
                column: 1,
            },
            host_state: ManifestRecord::default(),
        });
        let json = serde_json::to_string(&m).unwrap();
        let parsed: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, m);
    }

    #[test]
    fn helpers_resolve_uniformly_across_variants() {
        let s = Manifest::State(sample_state());
        let e = Manifest::Events(sample_events());
        assert_eq!(s.ogh_module(), "data/ui/chest.ogh");
        assert_eq!(e.ogh_module(), "data/ui/chest.ogh");
        assert_eq!(s.binding(), "untold_lore::ui::chest::ChestUiState");
        assert_eq!(e.binding(), "untold_lore::ui::chest::ChestUiMsg");
    }

    #[test]
    fn write_then_read_round_trips_on_disk() {
        let dir = tempdir();
        let path = dir.join("manifest.json");
        let original = Manifest::State(sample_state());
        original.write(&path).unwrap();
        let loaded = Manifest::read(&path).unwrap();
        assert_eq!(loaded, original);
    }

    #[test]
    fn write_creates_missing_parent_dirs() {
        let dir = tempdir();
        let path = dir.join("nested/dir/manifest.json");
        let original = Manifest::State(sample_state());
        original.write(&path).unwrap();
        assert!(path.exists());
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn read_missing_file_is_not_found() {
        let dir = tempdir();
        let path = dir.join("does-not-exist.json");
        let err = Manifest::read(&path).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn read_malformed_is_invalid_data() {
        let dir = tempdir();
        let path = dir.join("garbage.json");
        std::fs::write(&path, b"{ not valid json").unwrap();
        let err = Manifest::read(&path).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    /// Cheap tempdir without a tempfile dep — uses the standard
    /// `std::env::temp_dir()` plus a process-id + nanosecond
    /// counter for uniqueness. Test scope only.
    fn tempdir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path =
            std::env::temp_dir().join(format!("ogham-manifest-test-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    // Synthesize helper coverage lives in
    // `tests/binding_module_attr.rs` — the helpers' generic bound
    // requires `OghamState` / `OghamMsg` derives, and the derive
    // emits `::ogham::*` paths that only resolve from consumer
    // crates (i.e. integration tests), not from within the lib's
    // own `#[cfg(test)]` modules.
}
