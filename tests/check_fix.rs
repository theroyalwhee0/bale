//! Tests for check command --fix flag.

use std::fs;
use std::process::Command;
use tempfile::TempDir;

/// Test that --fix sorts an unsorted Central Directory.
#[test]
fn fix_sorts_unsorted_cd() {
    let temp_dir = TempDir::new().unwrap();
    let archive_path = temp_dir.path().join("test.bale");

    // Copy the unsorted fixture to temp location.
    fs::copy("tests/fixtures/invalid/unsorted_cd.bale", &archive_path).unwrap();

    // Run check without --fix first to verify it's unsorted.
    let output = Command::new(env!("CARGO_BIN_EXE_bale"))
        .args(["check", archive_path.to_str().unwrap()])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Central Directory is not sorted"),
        "Expected unsorted CD error, got: {}",
        stderr
    );

    // Run check with --fix.
    let output = Command::new(env!("CARGO_BIN_EXE_bale"))
        .args(["check", "--fix", archive_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success(), "check --fix should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("fixed: Sorted Central Directory"),
        "Expected fix message, got: {}",
        stdout
    );
    assert!(
        stdout.contains("Success (Compacted)"),
        "Expected Compacted status, got: {}",
        stdout
    );

    // Run check again to verify it's now sorted.
    let output = Command::new(env!("CARGO_BIN_EXE_bale"))
        .args(["check", archive_path.to_str().unwrap()])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stderr.contains("Central Directory is not sorted"),
        "CD should be sorted after fix, got stderr: {}",
        stderr
    );
    assert!(
        stdout.contains("Success (Compacted)"),
        "Expected Compacted status after fix, got: {}",
        stdout
    );
}

/// Test that --fix on an already sorted archive does nothing.
#[test]
fn fix_on_sorted_archive_does_nothing() {
    let temp_dir = TempDir::new().unwrap();
    let archive_path = temp_dir.path().join("test.bale");

    // Copy a valid compacted fixture.
    fs::copy("tests/fixtures/valid/single_file.bale", &archive_path).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_bale"))
        .args(["check", "--fix", archive_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should not contain any "fixed:" messages.
    assert!(
        !stdout.contains("fixed:"),
        "Should not fix anything on sorted archive, got: {}",
        stdout
    );
    assert!(
        stdout.contains("Success (Compacted)"),
        "Expected Compacted status, got: {}",
        stdout
    );
}
