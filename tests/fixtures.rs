//! Fixture generation tests.
//!
//! Run with `cargo test --test fixtures -- --ignored` to regenerate fixtures.

use std::fs::File;
use std::io::Write;
use std::path::Path;

use bale::{Archive, Eocd};
use zerocopy::IntoBytes;

const FIXTURES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

/// Generates an empty bale archive containing only the EOCD.
#[test]
#[ignore]
fn generate_empty_bale() {
    let path = Path::new(FIXTURES_DIR).join("empty.bale");
    let mut file = File::create(&path).expect("failed to create empty.bale");

    let eocd = Eocd::empty();
    file.write_all(eocd.as_bytes())
        .expect("failed to write EOCD");
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
    let mut archive = Archive::create(&archive_path).expect("failed to create archive");
    archive
        .add_file(&content_path, "hello.txt")
        .expect("failed to add file");
    archive.finish().expect("failed to finish archive");

    // Clean up source file.
    std::fs::remove_file(&content_path).expect("failed to remove hello.txt");
}
