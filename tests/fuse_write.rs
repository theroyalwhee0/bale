//! FUSE write operation integration tests.
//!
//! Tests file creation, deletion, and modification via `bale mount --shell`.

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

/// Create a file via FUSE mount.
#[test]
fn fuse_create_file() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");

    // Create archive with one file.
    run_bale(&["touch", archive.to_str().unwrap()]);

    // Mount and create a new file.
    let (success, stdout, _) = run_bale(&[
        "mount",
        archive.to_str().unwrap(),
        "--shell",
        "echo 'hello world' > newfile.txt && cat newfile.txt",
    ]);
    assert!(success, "mount --shell should succeed");
    assert!(
        stdout.contains("hello world"),
        "should read back created file"
    );

    // Verify file persists after unmount.
    let (success, stdout, _) = run_bale(&["ls", archive.to_str().unwrap()]);
    assert!(success);
    assert!(
        stdout.contains("newfile.txt"),
        "created file should persist"
    );
}

/// Create a directory via FUSE mount.
#[test]
fn fuse_mkdir() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");

    run_bale(&["touch", archive.to_str().unwrap()]);

    // Mount and create directory.
    let (success, _, _) = run_bale(&[
        "mount",
        archive.to_str().unwrap(),
        "--shell",
        "mkdir testdir && ls -d testdir",
    ]);
    assert!(success, "mkdir should succeed");

    // Verify directory persists.
    let (success, stdout, _) = run_bale(&["ls", archive.to_str().unwrap()]);
    assert!(success);
    assert!(
        stdout.contains("testdir"),
        "created directory should persist"
    );
}

/// Delete a file via FUSE mount.
#[test]
fn fuse_unlink() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");
    let file = dir.path().join("hello.txt");

    fs::write(&file, "hello").unwrap();
    run_bale(&["touch", archive.to_str().unwrap()]);
    run_bale(&["add", archive.to_str().unwrap(), file.to_str().unwrap()]);

    // Mount and delete file.
    let (success, _, _) = run_bale(&[
        "mount",
        archive.to_str().unwrap(),
        "--shell",
        "rm hello.txt",
    ]);
    assert!(success, "rm should succeed");

    // Verify file is gone.
    let (success, stdout, _) = run_bale(&["ls", archive.to_str().unwrap()]);
    assert!(success);
    assert!(!stdout.contains("hello.txt"), "deleted file should be gone");
}

/// Delete a directory via FUSE mount.
#[test]
fn fuse_rmdir() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");

    run_bale(&["touch", archive.to_str().unwrap()]);

    // Create and then remove directory.
    let (success, _, _) = run_bale(&[
        "mount",
        archive.to_str().unwrap(),
        "--shell",
        "mkdir testdir && rmdir testdir",
    ]);
    assert!(success, "mkdir && rmdir should succeed");

    // Verify directory is gone.
    let (success, stdout, _) = run_bale(&["ls", archive.to_str().unwrap()]);
    assert!(success);
    assert!(
        !stdout.contains("testdir"),
        "removed directory should be gone"
    );
}

/// Rename a file via FUSE mount.
#[test]
fn fuse_rename() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");
    let file = dir.path().join("original.txt");

    fs::write(&file, "content").unwrap();
    run_bale(&["touch", archive.to_str().unwrap()]);
    run_bale(&["add", archive.to_str().unwrap(), file.to_str().unwrap()]);

    // Mount and rename file.
    let (success, _, _) = run_bale(&[
        "mount",
        archive.to_str().unwrap(),
        "--shell",
        "mv original.txt renamed.txt",
    ]);
    assert!(success, "mv should succeed");

    // Verify rename persists.
    let (success, stdout, _) = run_bale(&["ls", archive.to_str().unwrap()]);
    assert!(success);
    assert!(
        !stdout.contains("original.txt"),
        "original name should be gone"
    );
    assert!(stdout.contains("renamed.txt"), "new name should exist");
}

/// Write to existing file via FUSE mount.
#[test]
fn fuse_write_existing() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");
    let file = dir.path().join("data.txt");

    fs::write(&file, "original").unwrap();
    run_bale(&["touch", archive.to_str().unwrap()]);
    run_bale(&["add", archive.to_str().unwrap(), file.to_str().unwrap()]);

    // Mount and overwrite file.
    let (success, stdout, _) = run_bale(&[
        "mount",
        archive.to_str().unwrap(),
        "--shell",
        "echo 'modified' > data.txt && cat data.txt",
    ]);
    assert!(success, "write should succeed");
    assert!(stdout.contains("modified"), "should read modified content");

    // Verify via extract.
    let out_dir = dir.path().join("out");
    fs::create_dir(&out_dir).unwrap();
    run_bale(&[
        "extract",
        archive.to_str().unwrap(),
        "-o",
        out_dir.to_str().unwrap(),
    ]);
    let content = fs::read_to_string(out_dir.join("data.txt")).unwrap();
    assert!(
        content.contains("modified"),
        "modified content should persist"
    );
}

/// Create nested directories via FUSE mount.
#[test]
fn fuse_mkdir_nested() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");

    run_bale(&["touch", archive.to_str().unwrap()]);

    // Mount and create nested directories.
    let (success, _, _) = run_bale(&[
        "mount",
        archive.to_str().unwrap(),
        "--shell",
        "mkdir -p a/b/c && echo 'deep' > a/b/c/file.txt",
    ]);
    assert!(success, "mkdir -p should succeed");

    // Verify structure persists.
    let (success, stdout, _) = run_bale(&["ls", archive.to_str().unwrap()]);
    assert!(success);
    assert!(
        stdout.contains("a/b/c/file.txt"),
        "nested file should exist"
    );
}

/// Symlink operations via FUSE mount.
#[test]
fn fuse_symlink() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");
    let file = dir.path().join("target.txt");

    fs::write(&file, "target content").unwrap();
    run_bale(&["touch", archive.to_str().unwrap()]);
    run_bale(&["add", archive.to_str().unwrap(), file.to_str().unwrap()]);

    // Mount and create symlink.
    let (success, stdout, _) = run_bale(&[
        "mount",
        archive.to_str().unwrap(),
        "--shell",
        "ln -s target.txt link.txt && cat link.txt",
    ]);
    assert!(success, "ln -s should succeed");
    assert!(
        stdout.contains("target content"),
        "should read through symlink"
    );

    // Verify symlink persists.
    let (success, stdout, _) = run_bale(&["ls", archive.to_str().unwrap()]);
    assert!(success);
    assert!(stdout.contains("link.txt"), "symlink should exist");
}

/// getattr returns correct file size after write.
#[test]
fn fuse_getattr_after_write() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");

    run_bale(&["touch", archive.to_str().unwrap()]);

    // Create file and check size.
    let (success, stdout, _) = run_bale(&[
        "mount",
        archive.to_str().unwrap(),
        "--shell",
        "echo -n '12345' > sized.txt && stat -c%s sized.txt",
    ]);
    assert!(success);
    assert!(stdout.trim() == "5", "file size should be 5 bytes");
}

/// Creating and writing a file should not create duplicate entries.
///
/// Regression test: create() adds an initial entry, then sync_modified_to_archive()
/// must delete before re-adding to avoid duplicates.
#[test]
fn fuse_no_duplicate_entries() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");

    run_bale(&["touch", archive.to_str().unwrap()]);

    // Create and write a file.
    let (success, _, _) = run_bale(&[
        "mount",
        archive.to_str().unwrap(),
        "--shell",
        "echo 'content' > file.txt",
    ]);
    assert!(success, "file creation should succeed");

    // Check for duplicates (orphaned data warning is OK).
    let (_, _, stderr) = run_bale(&["check", archive.to_str().unwrap()]);
    assert!(
        !stderr.contains("Duplicate"),
        "should not have duplicate entries: {stderr}"
    );

    // Should have exactly one entry.
    let (success, stdout, _) = run_bale(&["ls", archive.to_str().unwrap()]);
    assert!(success);
    let file_count = stdout.lines().filter(|l| l.contains("file.txt")).count();
    assert_eq!(file_count, 1, "should have exactly one file.txt entry");
}

/// setattr (chmod) via FUSE mount.
#[test]
fn fuse_setattr_chmod() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("test.bale");
    let file = dir.path().join("script.sh");

    fs::write(&file, "#!/bin/bash").unwrap();
    run_bale(&["touch", archive.to_str().unwrap()]);
    run_bale(&["add", archive.to_str().unwrap(), file.to_str().unwrap()]);

    // Mount and chmod.
    let (success, stdout, _) = run_bale(&[
        "mount",
        archive.to_str().unwrap(),
        "--shell",
        "chmod 755 script.sh && stat -c%a script.sh",
    ]);
    assert!(success, "chmod should succeed");
    assert!(stdout.trim() == "755", "mode should be 755");
}
