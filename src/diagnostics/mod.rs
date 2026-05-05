//! Schema-Diagnostic Surface — cross-side `.ogh` ↔ Rust drift
//! detection.
//!
//! See [`docs/internal/SCHEMA_DIAGNOSTICS.md`](../../docs/internal/SCHEMA_DIAGNOSTICS.md)
//! for the design contract and
//! [`docs/internal/SCHEMA_DIAGNOSTICS_IMPLEMENTATION.md`](../../docs/internal/SCHEMA_DIAGNOSTICS_IMPLEMENTATION.md)
//! for the per-merge implementation plan.
//!
//! ## Modules
//!
//! - [`manifest`] — on-disk JSON shape of a binding manifest. Each
//!   `#[derive(OghamState)]` / `#[derive(OghamMsg)]` declaring
//!   `#[ogham(binding_module = "...")]` writes one of these at
//!   proc-macro expansion time (P0-M3); the diagnostic backend
//!   (P0-M4) reads them and runs schema-match against a parsed
//!   [`crate::runtime::schema::ModuleSchema`].

pub mod check;
pub mod diagnostic;
pub mod manifest;

pub use check::check_against_manifest;
pub use diagnostic::{Diagnostic, RelatedSpan, Severity, BINDING_CODE};
pub use manifest::{
    EventsManifest, Manifest, ManifestEvent, ManifestField, ManifestRecord, RustSourceLoc,
    StateManifest,
};
