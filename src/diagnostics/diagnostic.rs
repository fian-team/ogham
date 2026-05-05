//! Crate-level diagnostic type for the schema-diagnostic surface.
//!
//! These are the structured outputs of
//! [`check_against_manifest`](super::check::check_against_manifest).
//! Front-ends convert them into their own representations:
//!
//! - The CLI (P0-M5) renders text in `cargo --message-format=short`
//!   shape.
//! - The LSP (P1-M2) converts each `Diagnostic` into a
//!   `tower_lsp::Diagnostic`, mapping `primary` to a `Range` and
//!   `related` to `DiagnosticRelatedInformation`.
//!
//! The runtime wrapper around `check_schemas_match` flattens them
//! into a single string for `RuntimeError::SchemaMismatch`,
//! preserving the existing contract.

use std::path::PathBuf;

use crate::parser::span::Span;

/// A schema-diagnostic finding emitted by the backend.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Stable identifier for the finding's category. v1 uses a single
    /// `ogham:binding` code; future refinement may split per-subkind.
    pub code: String,
    /// Human-readable description. Multi-line messages are allowed
    /// (the LSP back-end stuffs them into the message field; see
    /// `format_diagnostic_message` in the existing LSP layer for
    /// precedent).
    pub message: String,
    /// Primary location in the `.ogh` file the finding pertains to.
    /// Spans of `Span::zero()` indicate "no specific position" and
    /// fall back to line 1 in front-ends.
    pub primary: Span,
    /// Optional pointers at related source — typically the Rust-side
    /// derive's location, when known. Empty in Phase 0 because stable
    /// proc-macro doesn't expose source paths.
    pub related: Vec<RelatedSpan>,
    /// Fully-qualified Rust binding path (e.g.
    /// `untold_lore::ChestUiState`). Lets the LSP/CLI tag output by
    /// binding when multiple bindings target the same module — see
    /// `SCHEMA_DIAGNOSTICS.md`'s "all bindings must agree" rule (R2).
    pub binding_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// A pointer at related source, modelled on
/// [`tower_lsp::lsp_types::DiagnosticRelatedInformation`].
#[derive(Clone, Debug, PartialEq)]
pub struct RelatedSpan {
    pub file: PathBuf,
    pub line: u32,
    pub column: u32,
    pub message: String,
}

/// Single canonical code for binding-level findings. Front-ends can
/// match on this to filter or style appropriately.
pub const BINDING_CODE: &str = "ogham:binding";

impl Diagnostic {
    /// Build an `Error`-severity binding diagnostic with a primary
    /// span. Convenience helper for the most common case.
    pub fn binding_error(
        message: impl Into<String>,
        primary: Span,
        binding_id: Option<String>,
    ) -> Self {
        Self::binding(Severity::Error, message, primary, binding_id)
    }

    /// Build a `Warning`-severity binding diagnostic. Used by P1-M3
    /// (stale-manifest warnings) — defined here for symmetry now so
    /// callers don't have to reach for the struct literal.
    pub fn binding_warning(
        message: impl Into<String>,
        primary: Span,
        binding_id: Option<String>,
    ) -> Self {
        Self::binding(Severity::Warning, message, primary, binding_id)
    }

    /// Build an `Info`-severity binding diagnostic. Used by P1-M4
    /// (the "no manifest found" hint).
    pub fn binding_info(
        message: impl Into<String>,
        primary: Span,
        binding_id: Option<String>,
    ) -> Self {
        Self::binding(Severity::Info, message, primary, binding_id)
    }

    fn binding(
        severity: Severity,
        message: impl Into<String>,
        primary: Span,
        binding_id: Option<String>,
    ) -> Self {
        Self {
            severity,
            code: BINDING_CODE.to_string(),
            message: message.into(),
            primary,
            related: Vec::new(),
            binding_id,
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Error => f.write_str("error"),
            Severity::Warning => f.write_str("warning"),
            Severity::Info => f.write_str("info"),
        }
    }
}

impl std::fmt::Display for Diagnostic {
    /// Render in `cargo --message-format=short`-shaped form. The
    /// CLI in P0-M5 builds on top of this; the LSP doesn't use it
    /// (it goes through a structured `tower_lsp::Diagnostic`
    /// converter instead).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let line = if self.primary.start_line == 0 {
            1
        } else {
            self.primary.start_line
        };
        let col = if self.primary.start_column == 0 {
            1
        } else {
            self.primary.start_column
        };
        write!(
            f,
            "{line}:{col}: {sev}[{code}]: {msg}",
            sev = self.severity,
            code = self.code,
            msg = self.message,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_constructors_set_severity() {
        let span = Span::zero();
        let e = Diagnostic::binding_error("err", span, None);
        let w = Diagnostic::binding_warning("warn", span, None);
        let i = Diagnostic::binding_info("hint", span, None);
        assert_eq!(e.severity, Severity::Error);
        assert_eq!(w.severity, Severity::Warning);
        assert_eq!(i.severity, Severity::Info);
        // All carry the same code.
        assert_eq!(e.code, BINDING_CODE);
        assert_eq!(w.code, BINDING_CODE);
        assert_eq!(i.code, BINDING_CODE);
    }

    #[test]
    fn severity_display_is_lowercase_keyword() {
        assert_eq!(Severity::Error.to_string(), "error");
        assert_eq!(Severity::Warning.to_string(), "warning");
        assert_eq!(Severity::Info.to_string(), "info");
    }

    #[test]
    fn diagnostic_display_renders_cargo_short_shape() {
        let d = Diagnostic::binding_error(
            "field `x` missing",
            Span::new(7, 5, 7, 20),
            Some("test::State".into()),
        );
        let s = d.to_string();
        assert_eq!(s, "7:5: error[ogham:binding]: field `x` missing");
    }

    #[test]
    fn diagnostic_display_falls_back_to_line_1_for_zero_span() {
        let d = Diagnostic::binding_warning("stale", Span::zero(), None);
        let s = d.to_string();
        assert_eq!(s, "1:1: warning[ogham:binding]: stale");
    }
}
