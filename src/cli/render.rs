//! Text rendering for the `ogham check` CLI.
//!
//! Produces `cargo --message-format=short`-shaped output:
//!
//! ```text
//! data/ui/chest.ogh:14:5: error[ogham:binding]: host_state field
//!   `selected` declared in .ogh but missing from Rust binding ...
//!   --> src/ui/chest.rs:42:10
//!   <related message>
//! ```
//!
//! Multi-line messages have continuation lines indented two spaces.
//! Related spans with empty file paths (Phase 0 source-loc gap) are
//! skipped.

use std::io::Write;
use std::path::Path;

use crate::diagnostics::Diagnostic;

/// Render diagnostics for a single `.ogh` file path. Writes to the
/// provided writer (`stdout` in main, `&mut Vec<u8>` in tests).
pub fn render<W: Write>(
    out: &mut W,
    file_path: &Path,
    diags: &[Diagnostic],
) -> std::io::Result<()> {
    for d in diags {
        let line = if d.primary.start_line == 0 {
            1
        } else {
            d.primary.start_line
        };
        let col = if d.primary.start_column == 0 {
            1
        } else {
            d.primary.start_column
        };
        // Split the message on newlines so we can indent
        // continuations under the primary line.
        let mut lines = d.message.split('\n');
        let first = lines.next().unwrap_or("");
        writeln!(
            out,
            "{}:{}:{}: {}[{}]: {}",
            file_path.display(),
            line,
            col,
            d.severity,
            d.code,
            first,
        )?;
        for cont in lines {
            writeln!(out, "  {cont}")?;
        }
        for related in &d.related {
            if related.file.as_os_str().is_empty() {
                continue;
            }
            writeln!(
                out,
                "  --> {}:{}:{}",
                related.file.display(),
                related.line,
                related.column,
            )?;
            writeln!(out, "  {}", related.message)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{Diagnostic, RelatedSpan, Severity};
    use crate::parser::span::Span;
    use std::path::PathBuf;

    fn render_to_string(file_path: &Path, diags: &[Diagnostic]) -> String {
        let mut buf: Vec<u8> = Vec::new();
        render(&mut buf, file_path, diags).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn renders_single_line_diagnostic() {
        let d = Diagnostic::binding_error(
            "field `x` missing",
            Span::new(7, 5, 7, 20),
            Some("test::S".into()),
        );
        let out = render_to_string(Path::new("ui.ogh"), std::slice::from_ref(&d));
        assert_eq!(out, "ui.ogh:7:5: error[ogham:binding]: field `x` missing\n",);
    }

    #[test]
    fn renders_multi_line_message_with_indented_continuations() {
        let d = Diagnostic::binding_error(
            "type differs:\n  .ogh:  int\n  Rust:  string",
            Span::new(3, 1, 3, 5),
            None,
        );
        let out = render_to_string(Path::new("ui.ogh"), std::slice::from_ref(&d));
        assert_eq!(
            out,
            "ui.ogh:3:1: error[ogham:binding]: type differs:\n    .ogh:  int\n    Rust:  string\n",
        );
    }

    #[test]
    fn renders_related_spans_indented() {
        let mut d = Diagnostic::binding_error("primary", Span::zero(), None);
        d.related.push(RelatedSpan {
            file: PathBuf::from("src/foo.rs"),
            line: 42,
            column: 10,
            message: "originally declared here".into(),
        });
        let out = render_to_string(Path::new("ui.ogh"), std::slice::from_ref(&d));
        assert!(out.contains("--> src/foo.rs:42:10"));
        assert!(out.contains("originally declared here"));
    }

    #[test]
    fn skips_related_spans_with_empty_file() {
        let mut d = Diagnostic::binding_error("primary", Span::zero(), None);
        d.related.push(RelatedSpan {
            file: PathBuf::new(),
            line: 0,
            column: 0,
            message: "no source loc".into(),
        });
        let out = render_to_string(Path::new("ui.ogh"), std::slice::from_ref(&d));
        assert!(
            !out.contains("-->"),
            "should skip empty-file related spans, got: {out}"
        );
        assert!(!out.contains("no source loc"));
    }

    #[test]
    fn falls_back_to_line_1_when_span_is_zero() {
        let d = Diagnostic::binding_warning("stale", Span::zero(), None);
        let out = render_to_string(Path::new("ui.ogh"), std::slice::from_ref(&d));
        assert_eq!(out, "ui.ogh:1:1: warning[ogham:binding]: stale\n",);
    }

    #[test]
    fn renders_multiple_diagnostics_in_order() {
        let d1 = Diagnostic::binding_error("first", Span::new(1, 1, 1, 5), None);
        let d2 = Diagnostic::binding_warning("second", Span::new(2, 1, 2, 5), None);
        let out = render_to_string(Path::new("ui.ogh"), &[d1, d2]);
        let first_pos = out.find("first").unwrap();
        let second_pos = out.find("second").unwrap();
        assert!(first_pos < second_pos);
    }
}
