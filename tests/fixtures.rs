//! Fixture generation tests.
//!
//! Run with `cargo test --test fixtures -- --ignored` to regenerate fixtures.

use std::fs::File;
use std::io::Write;
use std::path::Path;

use bale::{ArchiveWriter, BaleEocd, Eocd};
use zerocopy::IntoBytes;

const FIXTURES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

/// Generates an empty bale archive with EOCD + BaleEocd (256 bytes).
#[test]
#[ignore]
fn generate_empty_bale() {
    let path = Path::new(FIXTURES_DIR).join("empty.bale");
    let mut file = File::create(&path).expect("failed to create empty.bale");

    let eocd = Eocd::new_with_comment(0, 0, 0, BaleEocd::SIZE as u16);
    let bale_eocd = BaleEocd::new();

    file.write_all(eocd.as_bytes())
        .expect("failed to write EOCD");
    file.write_all(bale_eocd.as_bytes())
        .expect("failed to write BaleEocd");
}

/// Generates a bale archive containing a single "hello.txt" file.
#[test]
#[ignore]
fn generate_single_file_bale() {
    let fixtures_dir = Path::new(FIXTURES_DIR);
    let archive_path = fixtures_dir.join("single_file.bale");
    let content_path = fixtures_dir.join("hello.txt");

    // Create source file.
    let mut src = File::create(&content_path).expect("failed to create hello.txt");
    src.write_all(b"Hello, World!")
        .expect("failed to write content");
    drop(src);

    // Remove existing archive if present.
    let _ = std::fs::remove_file(&archive_path);

    // Create archive.
    let mut writer = ArchiveWriter::create(&archive_path).expect("failed to create archive");
    writer
        .add_file(&content_path, "hello.txt")
        .expect("failed to add file");
    writer.sync().expect("failed to sync archive");

    // Clean up source file.
    std::fs::remove_file(&content_path).expect("failed to remove hello.txt");
}

/// Generates a bale archive with 16KB alignment.
#[test]
#[ignore]
fn generate_align_16k_bale() {
    let fixtures_dir = Path::new(FIXTURES_DIR);
    let archive_path = fixtures_dir.join("align_16k.bale");
    let content_path = fixtures_dir.join("align_test.txt");

    // Create source file.
    let mut src = File::create(&content_path).expect("failed to create source file");
    src.write_all(b"16KB alignment test")
        .expect("failed to write content");
    drop(src);

    // Remove existing archive if present.
    let _ = std::fs::remove_file(&archive_path);

    // Create archive with 16KB alignment.
    let mut writer = ArchiveWriter::create_with_options(&archive_path, 16384, 256)
        .expect("failed to create archive");
    writer
        .add_file(&content_path, "align_test.txt")
        .expect("failed to add file");
    writer.sync().expect("failed to sync archive");

    // Clean up source file.
    std::fs::remove_file(&content_path).expect("failed to remove source file");
}

/// Generates a bale archive with 2048-byte path size.
#[test]
#[ignore]
fn generate_path_2048_bale() {
    let fixtures_dir = Path::new(FIXTURES_DIR);
    let archive_path = fixtures_dir.join("path_2048.bale");
    let content_path = fixtures_dir.join("path_test.txt");

    // Create source file.
    let mut src = File::create(&content_path).expect("failed to create source file");
    src.write_all(b"2048-byte path size test")
        .expect("failed to write content");
    drop(src);

    // Remove existing archive if present.
    let _ = std::fs::remove_file(&archive_path);

    // Create archive with 2048-byte paths.
    let mut writer = ArchiveWriter::create_with_options(&archive_path, 4096, 2048)
        .expect("failed to create archive");
    writer
        .add_file(&content_path, "path_test.txt")
        .expect("failed to add file");
    writer.sync().expect("failed to sync archive");

    // Clean up source file.
    std::fs::remove_file(&content_path).expect("failed to remove source file");
}
