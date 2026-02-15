//! Shared helpers for FUSE integration tests.

#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command;

use tempfile::TempDir;

/// Runs the bale binary with the given arguments.
///
/// Returns `(success, stdout, stderr)`.
pub fn run_bale(args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_bale"))
        .args(args)
        .output()
        .expect("failed to execute bale");

    (
        output.status.success(),
        String::from_utf8(output.stdout).unwrap_or_default(),
        String::from_utf8(output.stderr).unwrap_or_default(),
    )
}

/// Copies a fixture archive to a unique temp directory and returns `(path, _guard)`.
///
/// The `TempDir` guard keeps the directory alive; drop it to clean up.
/// Each call gets its own directory so parallel tests never contend on
/// the same archive lock.
pub fn deploy_fixture(fixture: &str) -> (PathBuf, TempDir) {
    let dir = tempfile::Builder::new()
        .prefix("baletest-")
        .tempdir()
        .expect("failed to create temp dir");

    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(fixture);
    let dst = dir.path().join(src.file_name().unwrap());
    std::fs::copy(&src, &dst).expect("failed to copy fixture");
    (dst, dir)
}
