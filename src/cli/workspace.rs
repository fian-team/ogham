//! Workspace discovery — find binding manifests on disk and group
//! them by the absolute path of the `.ogh` module they target.
//!
//! Uses [`cargo_metadata`] to enumerate the workspace's target dir
//! and member crate roots from a starting directory. Each member's
//! `CARGO_MANIFEST_DIR` is the resolution context for its
//! manifests' relative `ogh_module` paths.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use cargo_metadata::MetadataCommand;

use crate::diagnostics::manifest::Manifest;

/// One discovered binding manifest, paired with its on-disk path
/// (kept so the staleness check can `stat` it).
#[derive(Debug, Clone)]
pub struct DiscoveredManifest {
    pub manifest: Manifest,
    pub manifest_path: PathBuf,
}

/// Registry keyed by canonical absolute path of the `.ogh` module.
/// Multiple manifests can target the same module — see the R2
/// "all bindings must agree" rule in `SCHEMA_DIAGNOSTICS.md`.
pub type Registry = HashMap<PathBuf, Vec<DiscoveredManifest>>;

/// Discover manifests by invoking `cargo metadata` from
/// `starting_dir`. Returns a registry keyed by absolute `.ogh` path.
///
/// Failures from `cargo metadata` (no Cargo.toml, cargo binary
/// missing) are surfaced as Err. Per-manifest read failures are
/// logged via `eprintln!` and the offending file is skipped — one
/// corrupt manifest shouldn't poison the whole run.
pub fn discover(starting_dir: &Path) -> Result<Registry, DiscoverError> {
    let metadata = MetadataCommand::new()
        .current_dir(starting_dir)
        .no_deps()
        .exec()
        .map_err(|e| DiscoverError::CargoMetadata(e.to_string()))?;

    // Collect each workspace member's crate root for ogh_module
    // resolution.
    let member_roots: Vec<PathBuf> = metadata
        .packages
        .iter()
        .filter(|p| metadata.workspace_members.contains(&p.id))
        .filter_map(|p| p.manifest_path.parent().map(|p| p.to_path_buf().into()))
        .collect();

    let target_dir: PathBuf = metadata.target_directory.into();
    let manifest_dir = target_dir.join("ogham");

    let mut registry: Registry = HashMap::new();

    if !manifest_dir.exists() {
        // No manifests yet — that's a clean state, not an error.
        return Ok(registry);
    }

    let entries = std::fs::read_dir(&manifest_dir).map_err(|e| {
        DiscoverError::Io(format!(
            "reading manifest dir {}: {e}",
            manifest_dir.display()
        ))
    })?;

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("ogham: skipping unreadable manifest entry: {e}");
                continue;
            }
        };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let manifest = match Manifest::read(&path) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("ogham: skipping malformed manifest {}: {e}", path.display());
                continue;
            }
        };
        let abs_ogh = match resolve_ogh_module(&manifest, &member_roots) {
            Some(p) => p,
            None => {
                // Manifest references an `.ogh` we can't locate —
                // either the file's been moved or the binding
                // points at a nonexistent path. Surface as a
                // warning but keep going.
                eprintln!(
                    "ogham: manifest {} references missing .ogh module `{}`",
                    path.display(),
                    manifest.ogh_module(),
                );
                continue;
            }
        };
        registry
            .entry(abs_ogh)
            .or_default()
            .push(DiscoveredManifest {
                manifest,
                manifest_path: path,
            });
    }

    Ok(registry)
}

/// Resolve a manifest's `ogh_module` (a crate-relative path) to its
/// absolute on-disk location by trying each workspace member root.
/// Returns the first candidate that actually exists. Canonicalizes
/// the result so the registry's keys are comparable across paths
/// that contain `.` or `..` segments.
fn resolve_ogh_module(manifest: &Manifest, member_roots: &[PathBuf]) -> Option<PathBuf> {
    let module = manifest.ogh_module();
    for root in member_roots {
        let candidate = root.join(module);
        if candidate.exists() {
            return candidate.canonicalize().ok();
        }
    }
    None
}

#[derive(Debug)]
pub enum DiscoverError {
    CargoMetadata(String),
    Io(String),
}

impl std::fmt::Display for DiscoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CargoMetadata(s) => write!(f, "cargo metadata failed: {s}"),
            Self::Io(s) => write!(f, "io error: {s}"),
        }
    }
}

impl std::error::Error for DiscoverError {}
