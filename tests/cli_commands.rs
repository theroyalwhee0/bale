//! Integration tests for CLI commands.
//!
//! These tests use actual bale command execution with temporary files.

use std::fs;
use std::process::Command;

use bale::{ArchiveWrite, ArchiveWriter};
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
    assert_eq!(meta.len(), 72, "empty archive should be 72 bytes");
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

/// List shows size 0 for empty archive.
#[test]
fn list_empty_archive_shows_size_zero() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");

    run_bale(&["touch", archive.to_str().unwrap()]);

    let (success, stdout, _) = run_bale(&["ls", archive.to_str().unwrap()]);
    assert!(success, "ls should succeed");
    assert!(
        stdout.starts_with("size 0\n"),
        "empty archive should show size 0, got: {stdout}"
    );
}

/// List shows size as sum of entry data sizes.
#[test]
fn list_shows_size_sum() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");
    let file1 = dir.path().join("hello.txt");
    let file2 = dir.path().join("empty.txt");

    fs::write(&file1, "Hello, World!").unwrap(); // 13 bytes
    fs::write(&file2, "").unwrap(); // 0 bytes

    run_bale(&["touch", archive.to_str().unwrap()]);
    run_bale(&["add", archive.to_str().unwrap(), file1.to_str().unwrap()]);
    run_bale(&["add", archive.to_str().unwrap(), file2.to_str().unwrap()]);

    let (success, stdout, _) = run_bale(&["ls", archive.to_str().unwrap()]);
    assert!(success, "ls should succeed");
    assert!(
        stdout.starts_with("size 13\n"),
        "size should be sum of file sizes (13), got: {stdout}"
    );
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
#[test]

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
#[test]

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
#[test]

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

/// Helper to create an archive with a directory entry via the library API.
fn create_archive_with_directory(archive_path: &std::path::Path) {
    let mut writer = ArchiveWriter::create(archive_path).unwrap();
    // Add a directory entry (mode 0o040755 = directory with rwxr-xr-x).
    writer.add_entry("mydir", b"", 0o040755).unwrap();
    // Add a file inside the directory.
    writer
        .add_entry("mydir/hello.txt", b"Hello!", 0o100644)
        .unwrap();
    writer.sync().unwrap();
}

/// Helper to create an archive with a symlink entry via the library API.
fn create_archive_with_symlink(archive_path: &std::path::Path) {
    let mut writer = ArchiveWriter::create(archive_path).unwrap();
    // Add a regular file.
    writer
        .add_entry("target.txt", b"link target content", 0o100644)
        .unwrap();
    // Add a symlink: data is the target path, mode 0o120777 = symlink.
    writer
        .add_entry("link.txt", b"target.txt", 0o120777)
        .unwrap();
    writer.sync().unwrap();
}

/// Extract creates directories from directory entries.
#[test]
fn extract_creates_directories() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");
    let out_dir = dir.path().join("output");

    create_archive_with_directory(&archive);

    let (success, stdout, stderr) = run_bale(&[
        "extract",
        archive.to_str().unwrap(),
        "-o",
        out_dir.to_str().unwrap(),
    ]);
    assert!(success, "extract should succeed: {stderr}");
    assert!(stdout.contains("extracted: mydir"), "should extract dir");
    assert!(
        stdout.contains("extracted: mydir/hello.txt"),
        "should extract file"
    );

    // Verify directory was created.
    assert!(
        out_dir.join("mydir").is_dir(),
        "mydir should be a directory"
    );
    // Verify file inside directory.
    assert_eq!(
        fs::read_to_string(out_dir.join("mydir/hello.txt")).unwrap(),
        "Hello!"
    );
}

/// Extract creates symlinks from symlink entries.
#[cfg(unix)]
#[test]
fn extract_creates_symlinks() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");
    let out_dir = dir.path().join("output");

    create_archive_with_symlink(&archive);

    let (success, stdout, stderr) = run_bale(&[
        "extract",
        archive.to_str().unwrap(),
        "-o",
        out_dir.to_str().unwrap(),
    ]);
    assert!(success, "extract should succeed: {stderr}");
    assert!(
        stdout.contains("extracted: link.txt -> target.txt"),
        "should show symlink target in output, got: {stdout}"
    );

    // Verify the symlink was created.
    let link = out_dir.join("link.txt");
    assert!(
        link.symlink_metadata().unwrap().file_type().is_symlink(),
        "link.txt should be a symlink"
    );
    assert_eq!(
        fs::read_link(&link).unwrap().to_str().unwrap(),
        "target.txt"
    );
    // Verify reading through the symlink works.
    assert_eq!(fs::read_to_string(&link).unwrap(), "link target content");
}

/// Extract with --flat strips directory components.
#[test]
fn extract_flat_strips_directories() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");
    let out_dir = dir.path().join("output");

    create_archive_with_directory(&archive);

    let (success, stdout, stderr) = run_bale(&[
        "extract",
        archive.to_str().unwrap(),
        "-o",
        out_dir.to_str().unwrap(),
        "--flat",
    ]);
    assert!(success, "extract --flat should succeed: {stderr}");

    // File should be extracted directly into output dir (no mydir/ prefix).
    let extracted = out_dir.join("hello.txt");
    assert!(
        extracted.exists(),
        "hello.txt should exist directly in output: {stdout}"
    );
    assert_eq!(fs::read_to_string(&extracted).unwrap(), "Hello!");

    // Directory entry should be skipped in flat mode.
    assert!(
        !out_dir.join("mydir").exists(),
        "directory entry should not be created in flat mode"
    );
}

/// Extract with --flat skips directory entries.
#[test]
fn extract_flat_skips_directory_entries() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");
    let out_dir = dir.path().join("output");

    // Create archive with only a directory entry.
    {
        let mut writer = ArchiveWriter::create(&archive).unwrap();
        writer.add_entry("emptydir", b"", 0o040755).unwrap();
        writer.sync().unwrap();
    }

    let (success, stdout, _) = run_bale(&[
        "extract",
        archive.to_str().unwrap(),
        "-o",
        out_dir.to_str().unwrap(),
        "--flat",
    ]);
    assert!(success, "extract --flat should succeed");
    // No files should be extracted.
    assert!(
        !stdout.contains("extracted:"),
        "no entries should be extracted in flat mode for directory-only archive, got: {stdout}"
    );
}

/// Extract with --flatten alias works (visible alias for --flat).
#[test]
fn extract_flatten_alias_works() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");
    let file1 = dir.path().join("hello.txt");
    let out_dir = dir.path().join("output");

    fs::write(&file1, "Hello!").unwrap();

    run_bale(&["touch", archive.to_str().unwrap()]);
    run_bale(&["add", archive.to_str().unwrap(), file1.to_str().unwrap()]);

    let (success, _, stderr) = run_bale(&[
        "extract",
        archive.to_str().unwrap(),
        "-o",
        out_dir.to_str().unwrap(),
        "--flatten",
    ]);
    assert!(success, "extract --flatten alias should succeed: {stderr}");
    assert!(out_dir.join("hello.txt").exists());
}

/// Extract skips existing files in non-interactive mode (no overwrite prompt).
#[test]
fn extract_skips_existing_in_non_interactive() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");
    let out_dir = dir.path().join("output");

    // Create archive with a file.
    {
        let mut writer = ArchiveWriter::create(&archive).unwrap();
        writer
            .add_entry("existing.txt", b"new content", 0o100644)
            .unwrap();
        writer.sync().unwrap();
    }

    // Pre-create the file with different content.
    fs::create_dir_all(&out_dir).unwrap();
    fs::write(out_dir.join("existing.txt"), "original content").unwrap();

    // Extract (non-interactive, so overwrite prompt defaults to "no").
    let (success, stdout, _) = run_bale(&[
        "extract",
        archive.to_str().unwrap(),
        "-o",
        out_dir.to_str().unwrap(),
    ]);
    assert!(success, "extract should succeed");
    assert!(
        stdout.contains("skipped: existing.txt"),
        "should report skipped file, got: {stdout}"
    );

    // Original content should be preserved.
    assert_eq!(
        fs::read_to_string(out_dir.join("existing.txt")).unwrap(),
        "original content"
    );
}
