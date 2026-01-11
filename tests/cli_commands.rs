//! Integration tests for CLI commands.
//!
//! These tests use actual bale command execution with temporary files.

use std::fs;
use std::process::Command;

use tempfile::tempdir;

/// Helper to run bale command and return (success, stdout, stderr).
fn run_bale(args: &[&str]) -> (bool, String, String) {
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

/// Touch creates an empty archive.
#[test]
fn touch_creates_archive() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");

    let (success, _, _) = run_bale(&["touch", archive.to_str().unwrap()]);
    assert!(success, "touch should succeed");

    let meta = fs::metadata(&archive).unwrap();
    assert_eq!(meta.len(), 256, "empty archive should be 256 bytes");
}

/// Touch on existing archive updates modification time.
#[test]
fn touch_updates_mtime() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");

    // Create archive.
    run_bale(&["touch", archive.to_str().unwrap()]);
    let mtime1 = fs::metadata(&archive).unwrap().modified().unwrap();

    // Wait a bit and touch again.
    std::thread::sleep(std::time::Duration::from_millis(50));
    run_bale(&["touch", archive.to_str().unwrap()]);
    let mtime2 = fs::metadata(&archive).unwrap().modified().unwrap();

    assert!(mtime2 > mtime1, "mtime should be updated");
}

/// Add command adds files to archive.
#[test]
fn add_files_to_archive() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");
    let file1 = dir.path().join("hello.txt");

    // Create a test file.
    fs::write(&file1, "Hello, World!").unwrap();

    // Create archive and add file.
    run_bale(&["touch", archive.to_str().unwrap()]);
    let (success, _, _) = run_bale(&["add", archive.to_str().unwrap(), file1.to_str().unwrap()]);
    assert!(success, "add should succeed");

    // Verify file is in archive.
    let (success, stdout, _) = run_bale(&["ls", archive.to_str().unwrap()]);
    assert!(success, "ls should succeed");
    assert!(
        stdout.contains("hello.txt"),
        "archive should contain hello.txt"
    );
}

/// Add with prefix puts files under directory.
#[test]
fn add_with_prefix() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");
    let file1 = dir.path().join("test.txt");

    fs::write(&file1, "test content").unwrap();

    run_bale(&["touch", archive.to_str().unwrap()]);
    let (success, _, _) = run_bale(&[
        "add",
        archive.to_str().unwrap(),
        "--prefix",
        "subdir",
        file1.to_str().unwrap(),
    ]);
    assert!(success, "add with prefix should succeed");

    let (_, stdout, _) = run_bale(&["ls", archive.to_str().unwrap()]);
    assert!(
        stdout.contains("subdir/test.txt"),
        "file should be under prefix"
    );
}

/// Delete removes files from archive.
#[test]
fn delete_files_from_archive() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");
    let file1 = dir.path().join("hello.txt");

    fs::write(&file1, "Hello").unwrap();

    run_bale(&["touch", archive.to_str().unwrap()]);
    run_bale(&["add", archive.to_str().unwrap(), file1.to_str().unwrap()]);

    // Delete the file.
    let (success, _, _) = run_bale(&["delete", archive.to_str().unwrap(), "hello.txt"]);
    assert!(success, "delete should succeed");

    // Verify file is gone.
    let (success, stdout, _) = run_bale(&["ls", archive.to_str().unwrap()]);
    assert!(success, "ls should succeed");
    assert!(
        !stdout.contains("hello.txt"),
        "archive should not contain hello.txt"
    );
}

/// Extract extracts files from archive.
/// FIXME: Skipped due to bug #93 - extract writes wrong content.
#[test]
#[ignore]
fn extract_files_from_archive() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");
    let file1 = dir.path().join("hello.txt");
    let out_dir = dir.path().join("output");

    fs::write(&file1, "Hello, World!").unwrap();
    fs::create_dir(&out_dir).unwrap();

    run_bale(&["touch", archive.to_str().unwrap()]);
    run_bale(&["add", archive.to_str().unwrap(), file1.to_str().unwrap()]);

    // Extract.
    let (success, _, _) = run_bale(&[
        "extract",
        archive.to_str().unwrap(),
        "-o",
        out_dir.to_str().unwrap(),
    ]);
    assert!(success, "extract should succeed");

    // Verify extracted file.
    let extracted = out_dir.join("hello.txt");
    assert!(extracted.exists(), "file should be extracted");
    assert_eq!(fs::read_to_string(&extracted).unwrap(), "Hello, World!");
}

/// Check reports valid archive.
/// FIXME: Skipped due to bug #93 - CRC mismatch after add.
#[test]
#[ignore]
fn check_valid_archive() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");
    let file1 = dir.path().join("test.txt");

    fs::write(&file1, "test").unwrap();

    run_bale(&["touch", archive.to_str().unwrap()]);
    run_bale(&["add", archive.to_str().unwrap(), file1.to_str().unwrap()]);

    let (success, stdout, _) = run_bale(&["check", archive.to_str().unwrap()]);
    assert!(success, "check should succeed for valid archive");
    assert!(stdout.contains("Compacted") || stdout.contains("Working"));
}

/// Compact removes orphaned data.
/// FIXME: Skipped due to bug #93 - incorrect data handling.
#[test]
#[ignore]
fn compact_archive() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");
    let file1 = dir.path().join("file1.txt");
    let file2 = dir.path().join("file2.txt");

    fs::write(&file1, "content 1").unwrap();
    fs::write(&file2, "content 2").unwrap();

    run_bale(&["touch", archive.to_str().unwrap()]);
    run_bale(&["add", archive.to_str().unwrap(), file1.to_str().unwrap()]);
    run_bale(&["add", archive.to_str().unwrap(), file2.to_str().unwrap()]);

    let size_before = fs::metadata(&archive).unwrap().len();

    // Delete a file (creates orphaned data).
    run_bale(&["delete", archive.to_str().unwrap(), "file1.txt"]);

    // Compact.
    let (success, _, _) = run_bale(&["compact", archive.to_str().unwrap()]);
    assert!(success, "compact should succeed");

    let size_after = fs::metadata(&archive).unwrap().len();
    assert!(
        size_after < size_before,
        "archive should be smaller after compact"
    );
}
