//! Cross-side schema-match backend.
//!
//! [`check_against_manifest`] is the single source of truth for
//! comparing a parsed `.ogh` [`ModuleSchema`] against a Rust-side
//! [`StateManifest`] / [`EventsManifest`] pair. It returns a flat
//! list of [`Diagnostic`]s — front-ends (CLI in P0-M5, LSP in
//! P1-M2) render them; the runtime wrapper in `typed.rs` renders
//! them as a single `RuntimeError::SchemaMismatch` string for
//! belt-and-suspenders runtime drift detection.
//!
//! ## What's checked
//!
//! - `host_state` field set: every `.ogh` field must exist on the
//!   Rust struct with the same `TypeRef`; every Rust field must
//!   appear in `.ogh`.
//! - Events: every `.ogh` event must exist in the Rust enum with
//!   matching arg types; every Rust variant must appear in `.ogh`.
//!
//! ## What's not checked
//!
//! - Records referenced by name (`TypeRef::Record("Item")`) aren't
//!   walked recursively; the `.ogh` resolver and the Rust trait
//!   already enforce that records exist on each side.
//! - Body type-checking of `.ogh` expressions — see
//!   [`SCHEMA_DIAGNOSTICS.md`](../../../docs/internal/SCHEMA_DIAGNOSTICS.md)'s
//!   non-goals.

use std::path::Path;

use crate::parser::span::Span;
use crate::parser::typed_bindings::TypeRef;
use crate::runtime::schema::{ModuleSchema, RecordSchema};

use super::diagnostic::Diagnostic;
use super::manifest::{EventsManifest, Manifest, ManifestField, StateManifest};

/// Compare a parsed `.ogh` module schema against optional Rust-side
/// state and events manifests, emitting one [`Diagnostic`] per
/// disagreement. The `binding_id` tags every emitted diagnostic so
/// multi-binding output (R2 rule) stays attributable to the right
/// Rust type.
///
/// Both manifests are optional. Pass `None` for each when no
/// Rust-side binding has been declared for that role — the function
/// emits no spurious diagnostics in that case.
pub fn check_against_manifest(
    parsed: &ModuleSchema,
    state: Option<&StateManifest>,
    events: Option<&EventsManifest>,
    binding_id: &str,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    if let Some(state) = state {
        check_state(parsed, state, binding_id, &mut diags);
    }
    if let Some(events) = events {
        check_events(parsed, events, binding_id, &mut diags);
    }
    diags
}

fn check_state(
    parsed: &ModuleSchema,
    manifest: &StateManifest,
    binding_id: &str,
    out: &mut Vec<Diagnostic>,
) {
    let bid = Some(binding_id.to_string());

    // Empty parsed.host_state + non-empty Rust state →
    // "host_state {} block missing from .ogh".
    if parsed.host_state.is_none() {
        if !manifest.host_state.fields.is_empty() {
            let names: Vec<&str> = manifest
                .host_state
                .fields
                .keys()
                .map(String::as_str)
                .collect();
            out.push(Diagnostic::binding_error(
                format!(
                    "Rust binding `{}` declares state fields ({}) but the .ogh module has no `host_state {{}}` block",
                    binding_id,
                    names.join(", "),
                ),
                Span::zero(),
                bid,
            ));
        }
        return;
    }

    let parsed_hs = parsed.host_state.as_ref().unwrap();

    for (name, parsed_field) in &parsed_hs.fields {
        match manifest.host_state.fields.get(name) {
            None => out.push(Diagnostic::binding_error(
                format!(
                    "host_state field `{name}` is declared in the .ogh module but missing from Rust binding `{binding_id}`"
                ),
                parsed_field.decl_span,
                bid.clone(),
            )),
            Some(manifest_field) => {
                if let Some(diag) = compare_field_ty(
                    name,
                    &parsed_field.ty,
                    manifest_field,
                    parsed_field.decl_span,
                    binding_id,
                ) {
                    out.push(diag);
                }
            }
        }
    }
    for name in manifest.host_state.fields.keys() {
        if !parsed_hs.fields.contains_key(name) {
            out.push(Diagnostic::binding_error(
                format!(
                    "host_state field `{name}` is on Rust binding `{binding_id}` but not declared in the .ogh module"
                ),
                fallback_span(parsed_hs),
                bid.clone(),
            ));
        }
    }
}

fn compare_field_ty(
    name: &str,
    parsed_ty: &TypeRef,
    manifest_field: &ManifestField,
    primary: Span,
    binding_id: &str,
) -> Option<Diagnostic> {
    let manifest_ty = match TypeRef::from_canonical_string(&manifest_field.ty) {
        Ok(t) => t,
        Err(e) => {
            return Some(Diagnostic::binding_error(
                format!(
                    "Rust binding `{binding_id}` carries malformed canonical type for field `{name}`: `{}` ({e})",
                    manifest_field.ty,
                ),
                primary,
                Some(binding_id.to_string()),
            ));
        }
    };
    if &manifest_ty != parsed_ty {
        return Some(Diagnostic::binding_error(
            format!(
                "host_state field `{name}` type differs:\n      .ogh:  {}\n      Rust:  {}",
                parsed_ty.to_canonical_string(),
                manifest_ty.to_canonical_string(),
            ),
            primary,
            Some(binding_id.to_string()),
        ));
    }
    None
}

fn fallback_span(parsed_hs: &RecordSchema) -> Span {
    parsed_hs.decl_span.unwrap_or(Span::zero())
}

/// Detect stale manifests — those whose Rust source has been
/// modified since the manifest was last written. Returns a
/// WARNING-severity diagnostic when staleness is detected.
///
/// In Phase 0 the manifest's `rust_source.file` is always empty
/// because stable proc-macro can't capture source paths; this
/// function is a no-op (returns `None`) in that case. Once source
/// paths land, the staleness check starts firing automatically
/// without further wiring.
///
/// `stat` failures on either path return `None` rather than an
/// error — staleness is a quality-of-life signal, not a hard
/// guarantee, and a missing source file is the user's signal that
/// something else is up.
pub fn check_staleness(manifest_path: &Path, manifest: &Manifest) -> Option<Diagnostic> {
    let rust_source = match manifest {
        Manifest::State(s) => &s.rust_source,
        Manifest::Events(e) => &e.rust_source,
    };
    if rust_source.file.is_empty() {
        return None;
    }
    let manifest_mtime = std::fs::metadata(manifest_path).ok()?.modified().ok()?;
    let rust_mtime = std::fs::metadata(&rust_source.file).ok()?.modified().ok()?;
    if rust_mtime > manifest_mtime {
        Some(Diagnostic::binding_warning(
            format!(
                "binding manifest at `{}` is older than its Rust source `{}` — diagnostics may be inaccurate; run `cargo check` to refresh",
                manifest_path.display(),
                rust_source.file,
            ),
            Span::zero(),
            Some(manifest.binding().to_string()),
        ))
    } else {
        None
    }
}

fn check_events(
    parsed: &ModuleSchema,
    manifest: &EventsManifest,
    binding_id: &str,
    out: &mut Vec<Diagnostic>,
) {
    let bid = Some(binding_id.to_string());

    for (name, parsed_sig) in &parsed.events {
        match manifest.events.get(name) {
            None => out.push(Diagnostic::binding_error(
                format!(
                    "event `{name}` is declared in the .ogh module but missing from Rust binding `{binding_id}`"
                ),
                parsed_sig.decl_span,
                bid.clone(),
            )),
            Some(manifest_sig) => {
                let parsed_args: Vec<String> = parsed_sig
                    .args
                    .iter()
                    .map(TypeRef::to_canonical_string)
                    .collect();
                // Parse manifest args and compare; collect any
                // canonical-string parse failures as their own
                // diagnostics rather than blanket-failing the event.
                let manifest_arg_results: Vec<Result<TypeRef, _>> = manifest_sig
                    .args
                    .iter()
                    .map(|s| TypeRef::from_canonical_string(s))
                    .collect();
                let parse_errors: Vec<&str> = manifest_sig
                    .args
                    .iter()
                    .zip(manifest_arg_results.iter())
                    .filter_map(|(s, r)| r.as_ref().err().map(|_| s.as_str()))
                    .collect();
                if !parse_errors.is_empty() {
                    out.push(Diagnostic::binding_error(
                        format!(
                            "Rust binding `{binding_id}` carries malformed canonical types for event `{name}` args: [{}]",
                            parse_errors.join(", "),
                        ),
                        parsed_sig.decl_span,
                        bid.clone(),
                    ));
                    continue;
                }
                let manifest_args: Vec<String> = manifest_arg_results
                    .iter()
                    .map(|r| r.as_ref().unwrap().to_canonical_string())
                    .collect();
                if parsed_args != manifest_args {
                    out.push(Diagnostic::binding_error(
                        format!(
                            "event `{name}` arg types differ:\n      .ogh:  ({})\n      Rust:  ({})",
                            parsed_args.join(", "),
                            manifest_args.join(", "),
                        ),
                        parsed_sig.decl_span,
                        bid.clone(),
                    ));
                }
            }
        }
    }
    for name in manifest.events.keys() {
        if !parsed.events.contains_key(name) {
            out.push(Diagnostic::binding_error(
                format!(
                    "event `{name}` is on Rust binding `{binding_id}` but not declared in the .ogh module"
                ),
                Span::zero(),
                bid.clone(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::manifest::{
        EventsManifest, ManifestEvent, ManifestField, ManifestRecord, RustSourceLoc, StateManifest,
    };
    use crate::parser::typed_bindings::{KeyType, PrimType, TypeRef};
    use crate::runtime::schema::{EventSig, FieldSchema, ModuleSchema, RecordSchema};
    use std::collections::BTreeMap;

    // ---- fixture helpers -----------------------------------------

    fn span() -> Span {
        Span::new(7, 5, 7, 20) // arbitrary non-zero span for primary-span asserts
    }

    fn empty_rs() -> RustSourceLoc {
        RustSourceLoc {
            file: String::new(),
            line: 0,
            column: 0,
        }
    }

    fn parsed_with_field(name: &str, ty: TypeRef) -> ModuleSchema {
        let mut fields = BTreeMap::new();
        fields.insert(
            name.to_string(),
            FieldSchema {
                ty,
                default: None,
                decl_span: span(),
            },
        );
        ModuleSchema {
            host_state: Some(RecordSchema {
                fields,
                decl_span: Some(span()),
            }),
            ..Default::default()
        }
    }

    fn state_manifest_with_field(name: &str, ty: &str) -> StateManifest {
        let mut fields = BTreeMap::new();
        fields.insert(
            name.to_string(),
            ManifestField { ty: ty.to_string() },
        );
        StateManifest {
            binding: "test::State".into(),
            ogh_module: "test.ogh".into(),
            rust_source: empty_rs(),
            host_state: ManifestRecord { fields },
        }
    }

    fn events_manifest_with(name: &str, args: Vec<&str>) -> EventsManifest {
        let mut events = BTreeMap::new();
        events.insert(
            name.to_string(),
            ManifestEvent {
                args: args.into_iter().map(String::from).collect(),
            },
        );
        EventsManifest {
            binding: "test::Msg".into(),
            ogh_module: "test.ogh".into(),
            rust_source: empty_rs(),
            events,
        }
    }

    // ---- state checks ---------------------------------------------

    #[test]
    fn empty_parsed_with_empty_manifest_emits_nothing() {
        let parsed = ModuleSchema::default();
        let state = StateManifest {
            binding: "test::Empty".into(),
            ogh_module: "test.ogh".into(),
            rust_source: empty_rs(),
            host_state: ManifestRecord::default(),
        };
        let diags = check_against_manifest(&parsed, Some(&state), None, "test::Empty");
        assert!(diags.is_empty(), "got: {diags:?}");
    }

    #[test]
    fn rust_state_without_ogh_host_state_emits_one_error() {
        let parsed = ModuleSchema::default();
        let state = state_manifest_with_field("selected", "int");
        let diags = check_against_manifest(&parsed, Some(&state), None, "test::State");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("no `host_state {}` block"));
        assert!(diags[0].message.contains("selected"));
        assert_eq!(diags[0].binding_id.as_deref(), Some("test::State"));
    }

    #[test]
    fn field_in_ogh_missing_from_rust_emits_error_with_decl_span() {
        let parsed = parsed_with_field("selected", TypeRef::Primitive(PrimType::Int));
        let empty_state = StateManifest {
            binding: "test::State".into(),
            ogh_module: "test.ogh".into(),
            rust_source: empty_rs(),
            host_state: ManifestRecord::default(),
        };
        let diags = check_against_manifest(&parsed, Some(&empty_state), None, "test::State");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("missing from Rust binding"));
        assert!(diags[0].message.contains("selected"));
        assert_eq!(diags[0].primary, span());
    }

    #[test]
    fn field_in_rust_missing_from_ogh_emits_error() {
        let parsed = ModuleSchema {
            host_state: Some(RecordSchema {
                fields: BTreeMap::new(),
                decl_span: Some(span()),
            }),
            ..Default::default()
        };
        let state = state_manifest_with_field("extra", "int");
        let diags = check_against_manifest(&parsed, Some(&state), None, "test::State");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("but not declared in the .ogh"));
        assert!(diags[0].message.contains("extra"));
    }

    #[test]
    fn type_mismatch_on_same_field_name_emits_error() {
        let parsed = parsed_with_field("count", TypeRef::Primitive(PrimType::Int));
        let state = state_manifest_with_field("count", "string");
        let diags = check_against_manifest(&parsed, Some(&state), None, "test::State");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("type differs"));
        assert!(diags[0].message.contains(".ogh:  int"));
        assert!(diags[0].message.contains("Rust:  string"));
    }

    #[test]
    fn malformed_canonical_string_in_state_manifest_emits_dedicated_error() {
        let parsed = parsed_with_field("count", TypeRef::Primitive(PrimType::Int));
        // "List[Item]" is the wrong syntax — we use array<T>, not List[T].
        let state = state_manifest_with_field("count", "List[Item]");
        let diags = check_against_manifest(&parsed, Some(&state), None, "test::State");
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("malformed canonical type"),
            "got: {}",
            diags[0].message,
        );
    }

    #[test]
    fn matching_state_emits_nothing() {
        let parsed = parsed_with_field(
            "items",
            TypeRef::Array(Box::new(TypeRef::Record("Item".into()))),
        );
        let state = state_manifest_with_field("items", "array<Item>");
        let diags = check_against_manifest(&parsed, Some(&state), None, "test::State");
        assert!(diags.is_empty(), "got: {diags:?}");
    }

    // ---- event checks ---------------------------------------------

    #[test]
    fn event_in_ogh_missing_from_rust_emits_error() {
        let mut events = BTreeMap::new();
        events.insert(
            "open".to_string(),
            EventSig {
                args: vec![],
                decl_span: span(),
            },
        );
        let parsed = ModuleSchema {
            events,
            ..Default::default()
        };
        let manifest = EventsManifest {
            binding: "test::Msg".into(),
            ogh_module: "test.ogh".into(),
            rust_source: empty_rs(),
            events: BTreeMap::new(),
        };
        let diags = check_against_manifest(&parsed, None, Some(&manifest), "test::Msg");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("event `open`"));
        assert!(diags[0].message.contains("missing from Rust binding"));
    }

    #[test]
    fn event_in_rust_missing_from_ogh_emits_error() {
        let parsed = ModuleSchema::default();
        let manifest = events_manifest_with("close", vec![]);
        let diags = check_against_manifest(&parsed, None, Some(&manifest), "test::Msg");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("event `close`"));
        assert!(diags[0].message.contains("not declared in the .ogh"));
    }

    #[test]
    fn event_with_arg_mismatch_emits_error_with_arg_lists() {
        let mut events = BTreeMap::new();
        events.insert(
            "take".to_string(),
            EventSig {
                args: vec![TypeRef::Primitive(PrimType::Int)],
                decl_span: span(),
            },
        );
        let parsed = ModuleSchema {
            events,
            ..Default::default()
        };
        let manifest = events_manifest_with("take", vec!["string"]);
        let diags = check_against_manifest(&parsed, None, Some(&manifest), "test::Msg");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("arg types differ"));
        assert!(diags[0].message.contains("(int)"));
        assert!(diags[0].message.contains("(string)"));
    }

    #[test]
    fn event_with_malformed_arg_emits_dedicated_error() {
        let mut events = BTreeMap::new();
        events.insert(
            "take".to_string(),
            EventSig {
                args: vec![TypeRef::Primitive(PrimType::Int)],
                decl_span: span(),
            },
        );
        let parsed = ModuleSchema {
            events,
            ..Default::default()
        };
        let manifest = events_manifest_with("take", vec!["BogusType[X]"]);
        let diags = check_against_manifest(&parsed, None, Some(&manifest), "test::Msg");
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("malformed canonical types"),
            "got: {}",
            diags[0].message,
        );
    }

    // ---- multi-binding -------------------------------------------

    #[test]
    fn multi_binding_calls_tag_diagnostics_independently() {
        let parsed = parsed_with_field("v", TypeRef::Primitive(PrimType::Int));
        let agreeing = state_manifest_with_field("v", "int");
        let mut disagreeing = state_manifest_with_field("v", "string");
        disagreeing.binding = "other::OtherState".into();

        let mut all = Vec::new();
        all.extend(check_against_manifest(
            &parsed,
            Some(&agreeing),
            None,
            "test::State",
        ));
        all.extend(check_against_manifest(
            &parsed,
            Some(&disagreeing),
            None,
            "other::OtherState",
        ));

        // Only the disagreeing binding produces output; it carries
        // its own binding_id.
        assert_eq!(all.len(), 1);
        assert_eq!(
            all[0].binding_id.as_deref(),
            Some("other::OtherState"),
            "expected diagnostics tagged with the disagreeing binding",
        );
    }

    // ---- container-type round-trip --------------------------------

    #[test]
    fn map_type_round_trips_through_canonical_string() {
        let parsed = parsed_with_field(
            "by_name",
            TypeRef::Map(KeyType::String, Box::new(TypeRef::Primitive(PrimType::Int))),
        );
        let state = state_manifest_with_field("by_name", "map<string, int>");
        let diags = check_against_manifest(&parsed, Some(&state), None, "test::State");
        assert!(diags.is_empty(), "got: {diags:?}");
    }

    // ---- matching event sets emit nothing -------------------------

    #[test]
    fn matching_events_emit_nothing() {
        let mut events = BTreeMap::new();
        events.insert(
            "open".to_string(),
            EventSig {
                args: vec![],
                decl_span: span(),
            },
        );
        events.insert(
            "take".to_string(),
            EventSig {
                args: vec![
                    TypeRef::Primitive(PrimType::Int),
                    TypeRef::Primitive(PrimType::String),
                ],
                decl_span: span(),
            },
        );
        let parsed = ModuleSchema {
            events,
            ..Default::default()
        };
        let mut manifest = events_manifest_with("open", vec![]);
        manifest.events.insert(
            "take".to_string(),
            ManifestEvent {
                args: vec!["int".into(), "string".into()],
            },
        );
        let diags = check_against_manifest(&parsed, None, Some(&manifest), "test::Msg");
        assert!(diags.is_empty(), "got: {diags:?}");
    }

    // ---- combined state + events both at once ---------------------

    #[test]
    fn check_runs_both_state_and_events_in_one_call() {
        // .ogh declares one host_state field + one event; the
        // manifests disagree on each. The single call should emit
        // diagnostics for both branches with the same binding_id.
        let mut hs_fields = BTreeMap::new();
        hs_fields.insert(
            "selected".to_string(),
            FieldSchema {
                ty: TypeRef::Primitive(PrimType::Int),
                default: None,
                decl_span: span(),
            },
        );
        let mut events = BTreeMap::new();
        events.insert(
            "open".to_string(),
            EventSig {
                args: vec![],
                decl_span: span(),
            },
        );
        let parsed = ModuleSchema {
            host_state: Some(RecordSchema {
                fields: hs_fields,
                decl_span: Some(span()),
            }),
            events,
            ..Default::default()
        };
        // State manifest disagrees (extra field); events manifest
        // disagrees (missing the open event).
        let state = state_manifest_with_field("extra_rust_only", "int");
        let manifest_events = EventsManifest {
            binding: "test::Msg".into(),
            ogh_module: "test.ogh".into(),
            rust_source: empty_rs(),
            events: BTreeMap::new(),
        };
        let diags = check_against_manifest(
            &parsed,
            Some(&state),
            Some(&manifest_events),
            "test::Combined",
        );
        // Three diagnostics: missing `selected` from Rust state,
        // extra `extra_rust_only` in Rust state, missing `open`
        // event in Rust enum.
        assert_eq!(diags.len(), 3, "got: {diags:?}");
        // All carry the same binding_id.
        for d in &diags {
            assert_eq!(d.binding_id.as_deref(), Some("test::Combined"));
        }
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(messages.iter().any(|m| m.contains("selected") && m.contains("missing from Rust")));
        assert!(messages.iter().any(|m| m.contains("extra_rust_only") && m.contains("not declared in the .ogh")));
        assert!(messages.iter().any(|m| m.contains("event `open`") && m.contains("missing from Rust")));
    }

    // ---- staleness check (P0-M5) ----------------------------------

    fn temp_path(name: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "ogham-staleness-test-{}-{}",
            std::process::id(),
            n,
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn make_state_manifest_with_source(file: &str) -> Manifest {
        let mut m = StateManifest {
            binding: "test::S".into(),
            ogh_module: "test.ogh".into(),
            rust_source: empty_rs(),
            host_state: ManifestRecord::default(),
        };
        m.rust_source.file = file.to_string();
        Manifest::State(m)
    }

    #[test]
    fn staleness_skips_when_source_path_empty() {
        // Phase 0: stable proc-macro can't capture source paths,
        // so check_staleness must no-op rather than spuriously
        // flagging every manifest.
        let manifest_path = temp_path("manifest.json");
        std::fs::write(&manifest_path, b"{}").unwrap();
        let manifest = Manifest::State(StateManifest {
            binding: "test::S".into(),
            ogh_module: "test.ogh".into(),
            rust_source: empty_rs(),
            host_state: ManifestRecord::default(),
        });
        assert!(check_staleness(&manifest_path, &manifest).is_none());
    }

    #[test]
    fn staleness_returns_none_when_source_missing() {
        // Stat failure on either side returns None — staleness is
        // best-effort, not a hard error path.
        let manifest_path = temp_path("manifest.json");
        std::fs::write(&manifest_path, b"{}").unwrap();
        let manifest = make_state_manifest_with_source("/does/not/exist.rs");
        assert!(check_staleness(&manifest_path, &manifest).is_none());
    }

    #[test]
    fn staleness_detected_when_rust_source_is_newer() {
        let manifest_path = temp_path("manifest.json");
        let rs_path = temp_path("source.rs");
        std::fs::write(&manifest_path, b"{}").unwrap();
        std::fs::write(&rs_path, b"fn main() {}").unwrap();
        // Force the manifest's mtime to be earlier than the rs's.
        let early = std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs(1_000_000);
        let later = std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs(2_000_000);
        std::fs::File::open(&manifest_path)
            .unwrap()
            .set_modified(early)
            .unwrap();
        std::fs::File::open(&rs_path)
            .unwrap()
            .set_modified(later)
            .unwrap();
        let manifest = make_state_manifest_with_source(&rs_path.to_string_lossy());
        let diag = check_staleness(&manifest_path, &manifest)
            .expect("expected staleness diagnostic");
        assert_eq!(diag.severity, crate::diagnostics::diagnostic::Severity::Warning);
        assert!(diag.message.contains("older than its Rust source"));
    }

    #[test]
    fn staleness_returns_none_when_manifest_is_newer() {
        let manifest_path = temp_path("manifest.json");
        let rs_path = temp_path("source.rs");
        std::fs::write(&manifest_path, b"{}").unwrap();
        std::fs::write(&rs_path, b"fn main() {}").unwrap();
        let early = std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs(1_000_000);
        let later = std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs(2_000_000);
        std::fs::File::open(&rs_path)
            .unwrap()
            .set_modified(early)
            .unwrap();
        std::fs::File::open(&manifest_path)
            .unwrap()
            .set_modified(later)
            .unwrap();
        let manifest = make_state_manifest_with_source(&rs_path.to_string_lossy());
        assert!(check_staleness(&manifest_path, &manifest).is_none());
    }
}
