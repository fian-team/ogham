//! Schema-diagnostic manifest emit (P0-M3).
//!
//! When a `#[derive(OghamState)]` or `#[derive(OghamMsg)]` is annotated
//! with `#[ogham(binding_module = "path/to/module.ogh")]`, this module
//! writes a JSON manifest describing the binding to
//! `<target>/ogham/<binding-id>.json` at proc-macro expansion time.
//!
//! The manifest format and types live in `ogham::diagnostics::manifest`
//! (the `ogham` crate); they're not directly used here because
//! `ogham-derive` is a proc-macro crate and adding `ogham` as a
//! dependency would be circular. Instead, we hand-write the JSON in a
//! shape that round-trips through serde-deserialized `Manifest`. The
//! integration tests verify the round-trip property.
//!
//! ## Type-mapping fidelity
//!
//! The macro statically maps `syn::Type` → canonical-string by
//! recognizing the same set of types Ogham's `OghamField` impls cover
//! (i32/i64/u32/usize → `int`, f32/f64 → `float`, bool → `bool`,
//! String → `string`, `Vec<T>` → `array<T>`, `Option<T>` → `T?`,
//! `HashMap<String|Int, V>` → `map<string|int, V>`). Any other type
//! falls back to its plain ident, treated as a `Record` reference.
//!
//! User-defined newtypes with custom `OghamField` impls (e.g. a
//! `Quality(f32)` that maps to `Primitive(Float)`) will be classified
//! as `Record("Quality")` here — the runtime check is still authoritative;
//! the manifest is a static approximation. If a manifest disagrees with
//! the runtime, the LSP surfaces it as a binding-mismatch diagnostic
//! and the user can opt out per-document or use a primitive type
//! directly. Documented limitation; revisit if real cases pile up.
//!
//! ## Source location
//!
//! Stable Rust proc-macros cannot read the source file path or
//! line/column of the derive site. We populate `RustSourceLoc` with
//! empty/zero values; once `proc_macro_span` stabilizes (or we adopt
//! a nightly feature), we can fill these in. Empty here is honest:
//! callers know the data isn't available rather than receiving a
//! plausible-looking lie.

use proc_macro2::TokenStream;
use quote::ToTokens;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use syn::{GenericArgument, PathArguments, Type, TypePath};

/// Per-field metadata threaded into a state manifest.
pub(crate) struct StateField {
    pub name: String,
    pub ty: Type,
}

/// Per-event metadata threaded into an events manifest.
pub(crate) struct EventSig {
    pub name: String,
    pub args: Vec<Type>,
}

/// Emit a state manifest for one `OghamState` derive. The fields are
/// already filtered by `#[ogham(skip)]` and renamed per
/// `#[ogham(rename = "...")]`. Failures (filesystem, JSON shape) are
/// logged via `eprintln!` and ignored — never breaks compilation.
pub(crate) fn emit_state_manifest(type_ident: &str, binding_module: &str, fields: &[StateField]) {
    if skip_emit() {
        return;
    }
    let crate_name = pkg_name();
    let binding = format!("{crate_name}::{type_ident}");
    let json = build_state_json(&binding, binding_module, fields);
    let path = manifest_path("state", &crate_name, binding_module, type_ident);
    if let Err(e) = atomic_write(&path, &json) {
        eprintln!(
            "ogham-derive: failed to write state manifest at {}: {e}",
            path.display(),
        );
    }
}

/// Emit an events manifest for one `OghamMsg` derive.
pub(crate) fn emit_events_manifest(type_ident: &str, binding_module: &str, events: &[EventSig]) {
    if skip_emit() {
        return;
    }
    let crate_name = pkg_name();
    let binding = format!("{crate_name}::{type_ident}");
    let json = build_events_json(&binding, binding_module, events);
    let path = manifest_path("events", &crate_name, binding_module, type_ident);
    if let Err(e) = atomic_write(&path, &json) {
        eprintln!(
            "ogham-derive: failed to write events manifest at {}: {e}",
            path.display(),
        );
    }
}

// ---------------------------------------------------------------------
// JSON construction
// ---------------------------------------------------------------------

fn build_state_json(binding: &str, ogh_module: &str, fields: &[StateField]) -> String {
    let mut field_pairs: Vec<(String, String)> = fields
        .iter()
        .map(|f| (f.name.clone(), syn_type_to_canonical(&f.ty)))
        .collect();
    // BTreeMap-equivalent ordering — matches the deserialized form's
    // field iteration order.
    field_pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let fields_json = field_pairs
        .iter()
        .map(|(name, ty)| format!(r#""{}":{{"ty":"{}"}}"#, json_escape(name), json_escape(ty),))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"kind":"state","binding":"{binding}","ogh_module":"{module}","rust_source":{src},"host_state":{{"fields":{{{fields_json}}}}}}}"#,
        binding = json_escape(binding),
        module = json_escape(ogh_module),
        src = empty_rust_source(),
    )
}

fn build_events_json(binding: &str, ogh_module: &str, events: &[EventSig]) -> String {
    let mut event_pairs: Vec<(String, Vec<String>)> = events
        .iter()
        .map(|e| {
            (
                e.name.clone(),
                e.args.iter().map(syn_type_to_canonical).collect(),
            )
        })
        .collect();
    event_pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let events_json = event_pairs
        .iter()
        .map(|(name, args)| {
            let args_json = args
                .iter()
                .map(|a| format!(r#""{}""#, json_escape(a)))
                .collect::<Vec<_>>()
                .join(",");
            format!(r#""{}":{{"args":[{args_json}]}}"#, json_escape(name),)
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"kind":"events","binding":"{binding}","ogh_module":"{module}","rust_source":{src},"events":{{{events_json}}}}}"#,
        binding = json_escape(binding),
        module = json_escape(ogh_module),
        src = empty_rust_source(),
    )
}

/// Empty `RustSourceLoc` — file unavailable on stable proc-macro,
/// line/column also unavailable. Once proc-macro span APIs stabilize
/// we can fill these in.
fn empty_rust_source() -> String {
    r#"{"file":"","line":0,"column":0}"#.to_string()
}

/// Map a syn::Type to its canonical-string TypeRef form. Recognizes
/// the same surface as Ogham's stdlib `OghamField` impls; everything
/// else falls back to `Record(<ident>)`. See module docs for the
/// "best-effort static approximation" caveat.
pub(crate) fn syn_type_to_canonical(ty: &Type) -> String {
    if let Some(s) = try_canonical(ty) {
        return s;
    }
    // Last-resort fallback: render the tokens compactly.
    let toks: TokenStream = ty.to_token_stream();
    toks.to_string().split_whitespace().collect::<String>()
}

fn try_canonical(ty: &Type) -> Option<String> {
    let TypePath { qself: None, path } = (match ty {
        Type::Path(p) => p,
        _ => return None,
    }) else {
        return None;
    };
    let last = path.segments.last()?;
    let name = last.ident.to_string();
    match (name.as_str(), &last.arguments) {
        (
            "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128"
            | "usize",
            PathArguments::None,
        ) => Some("int".into()),
        ("f32" | "f64", PathArguments::None) => Some("float".into()),
        ("bool", PathArguments::None) => Some("bool".into()),
        ("String", PathArguments::None) => Some("string".into()),
        ("Vec", PathArguments::AngleBracketed(args)) => {
            let inner = first_type_arg(args)?;
            Some(format!("array<{}>", syn_type_to_canonical(inner)))
        }
        ("Option", PathArguments::AngleBracketed(args)) => {
            let inner = first_type_arg(args)?;
            Some(format!("{}?", syn_type_to_canonical(inner)))
        }
        ("HashMap", PathArguments::AngleBracketed(args)) => {
            let mut tys = args.args.iter().filter_map(|a| match a {
                GenericArgument::Type(t) => Some(t),
                _ => None,
            });
            let key = tys.next()?;
            let val = tys.next()?;
            let key_str = syn_type_to_canonical(key);
            // Map keys must be `string` or `int` in Ogham's schema.
            // For other key types, fall back to Record-mode (loses
            // information but the runtime check will catch the
            // mismatch).
            if key_str != "string" && key_str != "int" {
                return None;
            }
            let val_str = syn_type_to_canonical(val);
            Some(format!("map<{key_str}, {val_str}>"))
        }
        // Plain identifier with no generics → treat as record.
        (other, PathArguments::None) => Some(other.to_string()),
        // Generic ident we don't recognize — fallback path takes over.
        _ => None,
    }
}

fn first_type_arg(args: &syn::AngleBracketedGenericArguments) -> Option<&Type> {
    args.args.iter().find_map(|a| match a {
        GenericArgument::Type(t) => Some(t),
        _ => None,
    })
}

/// JSON-escape a string. Handles backslash, double-quote, and the
/// control characters that JSON requires escapes for.
pub(crate) fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------
// Filesystem
// ---------------------------------------------------------------------

fn skip_emit() -> bool {
    std::env::var_os("OGHAM_SKIP_MANIFEST_EMIT").is_some()
}

fn pkg_name() -> String {
    std::env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "unknown".to_string())
}

fn target_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("CARGO_TARGET_DIR") {
        return PathBuf::from(custom);
    }
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(manifest_dir).join("target")
}

/// Build the manifest filename: `<kind>-<crate>-<sanitized-module>-<type>.json`.
/// Including the crate name keeps filenames distinct when two
/// workspace members both bind to the same `.ogh` module — the body
/// still tracks both bindings; the filenames just don't collide.
pub(crate) fn manifest_path(
    kind: &str,
    crate_name: &str,
    binding_module: &str,
    type_ident: &str,
) -> PathBuf {
    let sanitized = sanitize_for_path(binding_module);
    let crate_sanitized = sanitize_for_path(crate_name);
    let filename = format!("{kind}-{crate_sanitized}-{sanitized}-{type_ident}.json");
    target_dir().join("ogham").join(filename)
}

/// Replace path-unfriendly characters (`/`, `\`, `.`) with underscores.
pub(crate) fn sanitize_for_path(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | '.' => '_',
            other => other,
        })
        .collect()
}

/// Atomic write: serialize, write `<path>.tmp`, rename. Creates the
/// parent directory if needed. Failure is bubbled up to the caller,
/// which logs and continues.
fn atomic_write(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp_os: OsString = path.as_os_str().to_os_string();
    tmp_os.push(".tmp");
    let tmp_path = PathBuf::from(tmp_os);
    std::fs::write(&tmp_path, contents)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn json_escape_handles_specials() {
        assert_eq!(json_escape("plain"), "plain");
        assert_eq!(json_escape("with \"quote\""), "with \\\"quote\\\"");
        assert_eq!(json_escape("back\\slash"), "back\\\\slash");
        assert_eq!(json_escape("line\nbreak"), "line\\nbreak");
        assert_eq!(json_escape("tab\there"), "tab\\there");
        assert_eq!(json_escape("\x01"), "\\u0001");
    }

    #[test]
    fn sanitize_replaces_separators_and_dots() {
        assert_eq!(
            sanitize_for_path("data/engine/ui/chest.ogh"),
            "data_engine_ui_chest_ogh"
        );
        assert_eq!(sanitize_for_path("nested\\path.ogh"), "nested_path_ogh");
        assert_eq!(sanitize_for_path("plain"), "plain");
    }

    #[test]
    fn manifest_path_has_expected_shape() {
        // We can't easily inspect the absolute path because it's
        // env-dependent, but the *filename* component is deterministic.
        let p = manifest_path("state", "untold_lore", "data/ui/chest.ogh", "ChestUiState");
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(
            name,
            "state-untold_lore-data_ui_chest_ogh-ChestUiState.json"
        );
        let p = manifest_path("events", "untold_lore", "data/ui/chest.ogh", "ChestUiMsg");
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(name, "events-untold_lore-data_ui_chest_ogh-ChestUiMsg.json");
    }

    #[test]
    fn syn_type_canonicalizes_primitives() {
        let ty: Type = parse_quote!(i32);
        assert_eq!(syn_type_to_canonical(&ty), "int");
        let ty: Type = parse_quote!(i64);
        assert_eq!(syn_type_to_canonical(&ty), "int");
        let ty: Type = parse_quote!(usize);
        assert_eq!(syn_type_to_canonical(&ty), "int");
        let ty: Type = parse_quote!(f32);
        assert_eq!(syn_type_to_canonical(&ty), "float");
        let ty: Type = parse_quote!(f64);
        assert_eq!(syn_type_to_canonical(&ty), "float");
        let ty: Type = parse_quote!(bool);
        assert_eq!(syn_type_to_canonical(&ty), "bool");
        let ty: Type = parse_quote!(String);
        assert_eq!(syn_type_to_canonical(&ty), "string");
    }

    #[test]
    fn syn_type_canonicalizes_containers() {
        let ty: Type = parse_quote!(Vec<i32>);
        assert_eq!(syn_type_to_canonical(&ty), "array<int>");
        let ty: Type = parse_quote!(Option<String>);
        assert_eq!(syn_type_to_canonical(&ty), "string?");
        let ty: Type = parse_quote!(Vec<Option<f32>>);
        assert_eq!(syn_type_to_canonical(&ty), "array<float?>");
        let ty: Type = parse_quote!(HashMap<String, i32>);
        assert_eq!(syn_type_to_canonical(&ty), "map<string, int>");
    }

    #[test]
    fn syn_type_treats_unknown_idents_as_records() {
        let ty: Type = parse_quote!(Item);
        assert_eq!(syn_type_to_canonical(&ty), "Item");
        let ty: Type = parse_quote!(Vec<Player>);
        assert_eq!(syn_type_to_canonical(&ty), "array<Player>");
    }

    #[test]
    fn build_state_json_round_trips_through_serde_shape() {
        // Validate by parsing the JSON and asserting the expected
        // top-level keys + value shapes — emulates what
        // ogham::diagnostics::Manifest::read does (without that crate
        // available here).
        let fields = vec![
            StateField {
                name: "selected".into(),
                ty: parse_quote!(i32),
            },
            StateField {
                name: "items".into(),
                ty: parse_quote!(Vec<Item>),
            },
        ];
        let json = build_state_json("untold_lore::ChestUiState", "data/ui/chest.ogh", &fields);
        // Spot-check shape via substring matches; the integration
        // test in tests/binding_module_attr.rs runs the actual
        // round-trip through ogham::diagnostics::Manifest::read.
        assert!(json.contains(r#""kind":"state""#), "json: {json}");
        assert!(json.contains(r#""binding":"untold_lore::ChestUiState""#));
        assert!(json.contains(r#""ogh_module":"data/ui/chest.ogh""#));
        assert!(json.contains(r#""selected":{"ty":"int"}"#));
        assert!(json.contains(r#""items":{"ty":"array<Item>"}"#));
        assert!(json.contains(r#""rust_source":{"file":"","line":0,"column":0}"#));
    }

    #[test]
    fn build_events_json_round_trips_through_serde_shape() {
        let events = vec![
            EventSig {
                name: "open_chest".into(),
                args: vec![],
            },
            EventSig {
                name: "take_item".into(),
                args: vec![parse_quote!(i32), parse_quote!(Item)],
            },
        ];
        let json = build_events_json("untold_lore::ChestUiMsg", "data/ui/chest.ogh", &events);
        assert!(json.contains(r#""kind":"events""#));
        assert!(json.contains(r#""open_chest":{"args":[]}"#));
        assert!(json.contains(r#""take_item":{"args":["int","Item"]}"#));
    }

    #[test]
    fn hashmap_with_unsupported_key_falls_through_to_token_render() {
        // Ogham's schema only allows `string` or `int` map keys, so
        // `HashMap<bool, i32>` should fall through to the
        // last-resort `quote!()` rendering — which produces a string
        // the canonical-string parser then rejects, surfacing as a
        // `binding-malformed-canonical-type` diagnostic. Locks in
        // the contract so a future "be smart and try harder" change
        // doesn't silently start producing nonsense canonical strings.
        let ty: Type = parse_quote!(HashMap<bool, i32>);
        let rendered = syn_type_to_canonical(&ty);
        // The exact fallback text isn't important — what matters is
        // that it's NOT a valid canonical string. An identifier-only
        // record name is fine; a token-stream rendering is fine; the
        // backend will flag whichever non-parseable result we emit.
        assert!(
            rendered != "map<bool, int>",
            "key type bool must NOT silently become a valid map key (got: {rendered})",
        );
    }

    #[test]
    fn build_state_json_sorts_fields_alphabetically() {
        // Deterministic JSON output requires consistent field
        // ordering — must match the BTreeMap iteration on the read
        // side. Pass fields in reverse alphabetical order; assert
        // they appear sorted in the output.
        let fields = vec![
            StateField {
                name: "zeta".into(),
                ty: parse_quote!(i32),
            },
            StateField {
                name: "alpha".into(),
                ty: parse_quote!(bool),
            },
            StateField {
                name: "mid".into(),
                ty: parse_quote!(String),
            },
        ];
        let json = build_state_json("test::S", "test.ogh", &fields);
        let alpha_pos = json.find(r#""alpha""#).expect("alpha present");
        let mid_pos = json.find(r#""mid""#).expect("mid present");
        let zeta_pos = json.find(r#""zeta""#).expect("zeta present");
        assert!(alpha_pos < mid_pos, "alpha before mid in {json}");
        assert!(mid_pos < zeta_pos, "mid before zeta in {json}");
    }

    #[test]
    fn build_events_json_sorts_events_alphabetically() {
        let events = vec![
            EventSig {
                name: "zoom".into(),
                args: vec![],
            },
            EventSig {
                name: "abort".into(),
                args: vec![],
            },
        ];
        let json = build_events_json("test::M", "test.ogh", &events);
        let abort_pos = json.find(r#""abort""#).expect("abort present");
        let zoom_pos = json.find(r#""zoom""#).expect("zoom present");
        assert!(abort_pos < zoom_pos, "abort before zoom in {json}");
    }
}
