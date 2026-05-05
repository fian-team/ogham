//! `ogham` CLI — schema-diagnostic front-end.
//!
//! The `ogham` binary's `check` subcommand runs the diagnostic
//! backend ([`crate::diagnostics::check_against_manifest`]) against
//! one or more `.ogh` files and prints results in `cargo
//! --message-format=short` shape. Exits non-zero on any
//! ERROR-severity diagnostic.
//!
//! The CLI is the Phase 0 front-end; Phase 1 wires the same backend
//! through `ogham-lsp` for editor-time diagnostics. Both share
//! workspace discovery and staleness-detection logic from
//! [`crate::diagnostics`].

pub mod args;
pub mod check;
pub mod render;
pub mod workspace;
