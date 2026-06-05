//! `ogham check` subcommand body.
//!
//! Glues together [`crate::cli::workspace::discover`] (manifest
//! registry), [`crate::diagnostics::check_against_manifest`] (per-
//! binding diff), and [`crate::cli::render::render`] (text output).
//!
//! Exit codes follow the documented contract in
//! [`crate::cli::args::check_usage`]:
//! - `0` — no ERROR diagnostics
//! - `1` — at least one ERROR diagnostic
//! - `2` — usage / IO failure (returned via `Err` from `run`)

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::cli::args::CheckArgs;
use crate::cli::render;
use crate::cli::workspace::{discover, DiscoveredManifest, Registry};
use crate::diagnostics::{
    check::check_staleness, check_against_manifest, Diagnostic, Manifest, Severity,
};

/// Run the `check` subcommand. Returns the process exit code on
/// success (0 = clean, 1 = errors found) or a fatal error
/// description on usage / IO failure (mapped to exit 2 by the
/// caller).
pub fn run(args: CheckArgs) -> Result<i32, RunError> {
    let workspace_dir = match args.workspace.clone() {
        Some(dir) => dir,
        None => std::env::current_dir().map_err(|e| RunError::Io(format!("cwd: {e}")))?,
    };
    let registry = discover(&workspace_dir).map_err(|e| RunError::Discover(e.to_string()))?;

    let paths = if args.all {
        find_ogh_files(&workspace_dir).map_err(|e| RunError::Io(format!("walking cwd: {e}")))?
    } else {
        vec![args
            .path
            .clone()
            .expect("argparse guarantees path or --all")]
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut had_error = false;

    for path in paths {
        let abs = match path.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                return Err(RunError::Io(format!("resolving {}: {e}", path.display())));
            }
        };
        let diags = check_one(&abs, &registry, !args.no_staleness_check)?;
        if diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
            had_error = true;
        }
        render::render(&mut out, &path, &diags)
            .map_err(|e| RunError::Io(format!("writing output: {e}")))?;
    }
    out.flush()
        .map_err(|e| RunError::Io(format!("flushing output: {e}")))?;

    Ok(if had_error { 1 } else { 0 })
}

/// Diagnose a single `.ogh` file against any manifests in the
/// registry that target it.
fn check_one(
    abs: &Path,
    registry: &Registry,
    do_staleness: bool,
) -> Result<Vec<Diagnostic>, RunError> {
    let parsed = crate::runtime::schema::load_schema(abs)
        .map_err(|e| RunError::Schema(format!("{}: {e:?}", abs.display())))?;

    let empty = Vec::<DiscoveredManifest>::new();
    let manifests = registry.get(abs).unwrap_or(&empty);
    let mut diags = Vec::new();
    for dm in manifests {
        let binding_id = dm.manifest.binding();
        match &dm.manifest {
            Manifest::State(s) => {
                diags.extend(check_against_manifest(&parsed, Some(s), None, binding_id));
            }
            Manifest::Events(e) => {
                diags.extend(check_against_manifest(&parsed, None, Some(e), binding_id));
            }
        }
        if do_staleness {
            if let Some(d) = check_staleness(&dm.manifest_path, &dm.manifest) {
                diags.push(d);
            }
        }
    }
    Ok(diags)
}

/// Walk a directory recursively, collecting every `.ogh` file.
/// Used by `--all`. Skips `target/` directories so we don't pick up
/// build artifacts or vendored copies.
fn find_ogh_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    walk(root, &mut out)?;
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        if file_name == "target" || file_name == ".git" {
            continue;
        }
        let ty = entry.file_type()?;
        if ty.is_dir() {
            walk(&path, out)?;
        } else if ty.is_file() && path.extension().and_then(|s| s.to_str()) == Some("ogh") {
            out.push(path);
        }
    }
    Ok(())
}

#[derive(Debug)]
pub enum RunError {
    Io(String),
    Discover(String),
    Schema(String),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(s) => write!(f, "{s}"),
            Self::Discover(s) => write!(f, "manifest discovery failed: {s}"),
            Self::Schema(s) => write!(f, "could not parse .ogh: {s}"),
        }
    }
}

impl std::error::Error for RunError {}
