//! Integration tests for the `ogham check` CLI subcommand.
//!
//! Invokes the binary as a subprocess against fixture workspaces.
//! Each test sets up a self-contained tempdir with a Cargo.toml,
//! a `.ogh` source, and pre-baked manifests under `target/ogham/`.
//! We can't rely on the surrounding workspace's manifests because
//! they target paths under `data/ui/...` that don't exist as real
//! `.ogh` files.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn temp_workspace(name: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "ogham-cli-test-{}-{}-{}",
        std::process::id(),
        n,
        name,
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_minimal_workspace(dir: &Path, name: &str) {
    fs::write(
        dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "{name}"
version = "0.0.1"
edition = "2021"

[lib]
path = "lib.rs"
"#
        ),
    )
    .unwrap();
    fs::write(dir.join("lib.rs"), "").unwrap();
}

fn write_manifest(dir: &Path, filename: &str, body: &str) {
    let manifest_dir = dir.join("target").join("ogham");
    fs::create_dir_all(&manifest_dir).unwrap();
    fs::write(manifest_dir.join(filename), body).unwrap();
}

fn ogham_bin() -> &'static str {
    env!("CARGO_BIN_EXE_ogham")
}

fn run_check(workspace: &Path, args: &[&str]) -> std::process::Output {
    Command::new(ogham_bin())
        .args(["check"])
        .args(args)
        .current_dir(workspace)
        .output()
        .expect("failed to invoke ogham binary")
}

#[test]
fn clean_run_exits_zero_with_no_output() {
    let dir = temp_workspace("clean_zero");
    write_minimal_workspace(&dir, "clean_fixture");
    fs::write(
        dir.join("ui.ogh"),
        "host_state {\n    selected: int,\n};\n\nlet main = fn () { 42 };\n",
    )
    .unwrap();
    write_manifest(
        &dir,
        "state-clean_fixture-ui_ogh-State.json",
        r#"{"kind":"state","binding":"clean_fixture::State","ogh_module":"ui.ogh","rust_source":{"file":"","line":0,"column":0},"host_state":{"fields":{"selected":{"ty":"int"}}}}"#,
    );

    let output = run_check(&dir, &["ui.ogh"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    );
    assert!(output.stdout.is_empty(), "expected empty stdout on clean check; got: {}", String::from_utf8_lossy(&output.stdout));
}

#[test]
fn drift_exits_one_with_diagnostic_text() {
    let dir = temp_workspace("drift_one");
    write_minimal_workspace(&dir, "drift_fixture");
    fs::write(
        dir.join("ui.ogh"),
        "host_state {\n    selected: int,\n};\n\nlet main = fn () { 42 };\n",
    )
    .unwrap();
    // Manifest claims `selected: string` — disagrees with .ogh's
    // `selected: int`.
    write_manifest(
        &dir,
        "state-drift_fixture-ui_ogh-State.json",
        r#"{"kind":"state","binding":"drift_fixture::State","ogh_module":"ui.ogh","rust_source":{"file":"","line":0,"column":0},"host_state":{"fields":{"selected":{"ty":"string"}}}}"#,
    );

    let output = run_check(&dir, &["ui.ogh"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("error[ogham:binding]"), "got: {stdout}");
    assert!(stdout.contains("type differs"), "got: {stdout}");
    assert!(stdout.contains("`selected`"), "got: {stdout}");
    assert!(stdout.contains(".ogh:  int"), "got: {stdout}");
    assert!(stdout.contains("Rust:  string"), "got: {stdout}");
}

#[test]
fn all_walks_cwd_and_skips_target() {
    let dir = temp_workspace("all_walk");
    write_minimal_workspace(&dir, "all_fixture");
    // Two .ogh files, both clean.
    fs::write(
        dir.join("a.ogh"),
        "host_state { x: int };\n\nlet main = fn () { 42 };\n",
    )
    .unwrap();
    fs::write(
        dir.join("b.ogh"),
        "host_state { y: bool };\n\nlet main = fn () { 42 };\n",
    )
    .unwrap();
    write_manifest(
        &dir,
        "state-all_fixture-a_ogh-A.json",
        r#"{"kind":"state","binding":"all_fixture::A","ogh_module":"a.ogh","rust_source":{"file":"","line":0,"column":0},"host_state":{"fields":{"x":{"ty":"int"}}}}"#,
    );
    write_manifest(
        &dir,
        "state-all_fixture-b_ogh-B.json",
        r#"{"kind":"state","binding":"all_fixture::B","ogh_module":"b.ogh","rust_source":{"file":"","line":0,"column":0},"host_state":{"fields":{"y":{"ty":"bool"}}}}"#,
    );

    let output = run_check(&dir, &["--all"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    );
}

#[test]
fn bad_path_exits_two() {
    let dir = temp_workspace("bad_path");
    write_minimal_workspace(&dir, "bad_fixture");
    let output = run_check(&dir, &["does-not-exist.ogh"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    );
}

#[test]
fn usage_error_when_no_args() {
    let dir = temp_workspace("no_args");
    write_minimal_workspace(&dir, "noargs_fixture");
    let output = Command::new(ogham_bin())
        .current_dir(&dir)
        .output()
        .expect("failed to invoke ogham");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("subcommand"), "got: {stderr}");
}

#[test]
fn help_exits_zero() {
    let output = Command::new(ogham_bin())
        .arg("--help")
        .output()
        .expect("failed to invoke ogham");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Usage:"), "got: {stdout}");
}

#[test]
fn check_help_exits_zero() {
    let output = Command::new(ogham_bin())
        .args(["check", "--help"])
        .output()
        .expect("failed to invoke ogham");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("--all"), "got: {stdout}");
    assert!(stdout.contains("--no-staleness-check"), "got: {stdout}");
}

#[test]
fn no_staleness_flag_suppresses_staleness_warnings() {
    // P0-M5 ships staleness as a no-op when rust_source.file is
    // empty, so this test mainly verifies the flag is accepted and
    // doesn't crash. Real staleness coverage lives in the unit tests
    // for `check_staleness` (src/diagnostics/check.rs) where we can
    // manipulate mtimes directly.
    let dir = temp_workspace("no_staleness");
    write_minimal_workspace(&dir, "stale_fixture");
    fs::write(
        dir.join("ui.ogh"),
        "host_state { x: int };\n\nlet main = fn () { 42 };\n",
    )
    .unwrap();
    write_manifest(
        &dir,
        "state-stale_fixture-ui_ogh-State.json",
        r#"{"kind":"state","binding":"stale_fixture::State","ogh_module":"ui.ogh","rust_source":{"file":"","line":0,"column":0},"host_state":{"fields":{"x":{"ty":"int"}}}}"#,
    );

    let output = run_check(&dir, &["ui.ogh", "--no-staleness-check"]);
    assert_eq!(output.status.code(), Some(0));
}
