//! Virtual `.bale/` directory integration tests.
//!
//! Tests the virtual `.bale/` metadata directory exposed by the FUSE layer,
//! including directory listing, metadata file content, hidden entries,
//! symlink access from subdirectories, and write denial.

mod common;

use common::{deploy_fixture, run_bale};

const FIXTURE: &str = "tests/fixtures/fuse/fuse_virtual_bale.bale";

/// `.bale` is hidden from `ls` in root directory.
#[test]
fn dotbale_hidden_from_ls() {
    let (archive, _dir) = deploy_fixture(FIXTURE);
    let path = archive.to_str().unwrap();
    let (success, stdout, _) = run_bale(&["mount", path, "--read-only", "--shell", "ls -a"]);
    assert!(success, "mount --shell should succeed");
    assert!(
        !stdout.contains(".bale"),
        ".bale should not appear in ls output: {stdout}"
    );
}

/// `.bale` is hidden from `ls` in subdirectories too.
#[test]
fn dotbale_hidden_from_ls_subdir() {
    let (archive, _dir) = deploy_fixture(FIXTURE);
    let path = archive.to_str().unwrap();
    let (success, stdout, _) = run_bale(&["mount", path, "--read-only", "--shell", "ls -a subdir"]);
    assert!(success, "mount --shell should succeed");
    assert!(
        !stdout.contains(".bale"),
        ".bale should not appear in subdir ls output: {stdout}"
    );
}

/// `stat .bale` works (lookup resolves the hidden entry).
#[test]
fn dotbale_stat_works() {
    let (archive, _dir) = deploy_fixture(FIXTURE);
    let path = archive.to_str().unwrap();
    let (success, stdout, _) =
        run_bale(&["mount", path, "--read-only", "--shell", "stat -c%F .bale"]);
    assert!(success, "stat .bale should succeed");
    assert!(
        stdout.contains("directory"),
        "stat should report .bale as directory: {stdout}"
    );
}

/// `cat .bale/version` returns the format version.
#[test]
fn dotbale_version_content() {
    let (archive, _dir) = deploy_fixture(FIXTURE);
    let path = archive.to_str().unwrap();
    let (success, stdout, _) =
        run_bale(&["mount", path, "--read-only", "--shell", "cat .bale/version"]);
    assert!(success, "cat .bale/version should succeed");
    assert_eq!(stdout.trim(), "1.0.0", "version should be 1.0.0");
}

/// `cat .bale/entry_count` returns the correct count.
///
/// Fixture has: hello.txt, subdir/, subdir/nested/, subdir/nested/deep.txt = 4 entries.
#[test]
fn dotbale_entry_count_content() {
    let (archive, _dir) = deploy_fixture(FIXTURE);
    let path = archive.to_str().unwrap();
    let (success, stdout, _) = run_bale(&[
        "mount",
        path,
        "--read-only",
        "--shell",
        "cat .bale/entry_count",
    ]);
    assert!(success, "cat .bale/entry_count should succeed");
    assert_eq!(stdout.trim(), "4", "should have 4 entries");
}

/// `cat .bale/directory_count` returns the correct count.
///
/// Fixture has 4 directory table rows (one per path).
#[test]
fn dotbale_directory_count_content() {
    let (archive, _dir) = deploy_fixture(FIXTURE);
    let path = archive.to_str().unwrap();
    let (success, stdout, _) = run_bale(&[
        "mount",
        path,
        "--read-only",
        "--shell",
        "cat .bale/directory_count",
    ]);
    assert!(success, "cat .bale/directory_count should succeed");
    assert_eq!(stdout.trim(), "4", "should have 4 directory entries");
}

/// `cat .bale/path_size` returns the default path size.
#[test]
fn dotbale_path_size_content() {
    let (archive, _dir) = deploy_fixture(FIXTURE);
    let path = archive.to_str().unwrap();
    let (success, stdout, _) = run_bale(&[
        "mount",
        path,
        "--read-only",
        "--shell",
        "cat .bale/path_size",
    ]);
    assert!(success);
    assert_eq!(stdout.trim(), "256", "default path_size should be 256");
}

/// `cat .bale/alignment` returns the default alignment.
#[test]
fn dotbale_alignment_content() {
    let (archive, _dir) = deploy_fixture(FIXTURE);
    let path = archive.to_str().unwrap();
    let (success, stdout, _) = run_bale(&[
        "mount",
        path,
        "--read-only",
        "--shell",
        "cat .bale/alignment",
    ]);
    assert!(success);
    assert_eq!(stdout.trim(), "4096", "default alignment should be 4096");
}

/// `cat .bale/compacted` returns a boolean value.
#[test]
fn dotbale_compacted_content() {
    let (archive, _dir) = deploy_fixture(FIXTURE);
    let path = archive.to_str().unwrap();
    let (success, stdout, _) = run_bale(&[
        "mount",
        path,
        "--read-only",
        "--shell",
        "cat .bale/compacted",
    ]);
    assert!(success);
    let value = stdout.trim();
    assert!(
        value == "true" || value == "false",
        "compacted should be true or false, got: {value}"
    );
}

/// `.bale/version` is accessible from a subdirectory via symlink.
#[test]
fn dotbale_accessible_from_subdir() {
    let (archive, _dir) = deploy_fixture(FIXTURE);
    let path = archive.to_str().unwrap();
    let (success, stdout, _) = run_bale(&[
        "mount",
        path,
        "--read-only",
        "--shell",
        "cd subdir && cat .bale/version",
    ]);
    assert!(success, "cat .bale/version from subdir should succeed");
    assert_eq!(
        stdout.trim(),
        "1.0.0",
        "version from subdir should be 1.0.0"
    );
}

/// `.bale/version` is accessible from a nested subdirectory via symlink.
#[test]
fn dotbale_accessible_from_nested_subdir() {
    let (archive, _dir) = deploy_fixture(FIXTURE);
    let path = archive.to_str().unwrap();
    let (success, stdout, _) = run_bale(&[
        "mount",
        path,
        "--read-only",
        "--shell",
        "cd subdir/nested && cat .bale/version",
    ]);
    assert!(
        success,
        "cat .bale/version from nested subdir should succeed"
    );
    assert_eq!(
        stdout.trim(),
        "1.0.0",
        "version from nested subdir should be 1.0.0"
    );
}

/// `readlink .bale` in a subdirectory shows relative path.
#[test]
fn dotbale_readlink_in_subdir() {
    let (archive, _dir) = deploy_fixture(FIXTURE);
    let path = archive.to_str().unwrap();
    let (success, stdout, _) = run_bale(&[
        "mount",
        path,
        "--read-only",
        "--shell",
        "cd subdir && readlink .bale",
    ]);
    assert!(success, "readlink .bale should succeed");
    assert_eq!(
        stdout.trim(),
        "../.bale",
        "symlink should point to ../.bale"
    );
}

/// `readlink .bale` in a nested subdirectory shows correct depth.
#[test]
fn dotbale_readlink_in_nested_subdir() {
    let (archive, _dir) = deploy_fixture(FIXTURE);
    let path = archive.to_str().unwrap();
    let (success, stdout, _) = run_bale(&[
        "mount",
        path,
        "--read-only",
        "--shell",
        "cd subdir/nested && readlink .bale",
    ]);
    assert!(success, "readlink .bale should succeed");
    assert_eq!(
        stdout.trim(),
        "../../.bale",
        "symlink should point to ../../.bale"
    );
}

/// `ls .bale/` lists all metadata files.
#[test]
fn dotbale_ls_lists_all_files() {
    let (archive, _dir) = deploy_fixture(FIXTURE);
    let path = archive.to_str().unwrap();
    let (success, stdout, _) = run_bale(&["mount", path, "--read-only", "--shell", "ls .bale"]);
    assert!(success, "ls .bale should succeed");

    let expected = [
        "alignment",
        "archive_size",
        "compacted",
        "directory_count",
        "entry_count",
        "path_size",
        "version",
    ];
    for name in &expected {
        assert!(
            stdout.contains(name),
            "{name} should appear in ls .bale output: {stdout}"
        );
    }
}
