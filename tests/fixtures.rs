//! Fixture generation tests.
//!
//! Run with `cargo test --test fixtures -- --ignored` to regenerate fixtures.

use std::fs::File;
use std::io::Write;
use std::path::Path;

use bale::{ArchiveWrite, ArchiveWriter, FileHeader, Trailer};
use zerocopy::IntoBytes;

const VALID_FIXTURES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/valid");
const INVALID_FIXTURES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/invalid");

/// Regenerates all test fixtures.
///
/// Run with: `cargo test --test fixtures -- --ignored`
#[test]
#[ignore]
fn generate_all_fixtures() {
    generate_empty_bale();
    generate_single_file_bale();
    generate_multi_file_bale();
    generate_align_16k_bale();
    generate_path_2048_bale();
    generate_orphaned_data_bale();
    generate_unsorted_cd_bale();
    generate_duplicate_paths_bale();
    generate_bad_crc_bale();
}

/// Generates an empty bale archive (FileHeader + Trailer = 72 bytes).
fn generate_empty_bale() {
    let path = Path::new(VALID_FIXTURES_DIR).join("empty.bale");
    let mut file = File::create(&path).expect("failed to create empty.bale");

    // File header (8 bytes).
    let header = FileHeader::new();
    file.write_all(header.as_bytes())
        .expect("failed to write FileHeader");

    // Trailer (64 bytes) with default settings.
    let trailer = Trailer::new();
    file.write_all(trailer.as_bytes())
        .expect("failed to write Trailer");
}

/// Generates a bale archive containing a single "hello.txt" file.
fn generate_single_file_bale() {
    let fixtures_dir = Path::new(VALID_FIXTURES_DIR);
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

/// Generates a bale archive with multiple files and directories.
///
/// Contains various entry types for testing ls-style output:
/// - Directories (mode 0o040755)
/// - Regular files (mode 0o100644)
/// - Executable files (mode 0o100755)
/// - Read-only files (mode 0o100444)
fn generate_multi_file_bale() {
    let archive_path = Path::new(VALID_FIXTURES_DIR).join("multi_file.bale");

    // Remove existing archive if present.
    let _ = std::fs::remove_file(&archive_path);

    // Create archive with various entry types.
    let mut writer = ArchiveWriter::create(&archive_path).expect("failed to create archive");

    // Directories (note: directories have no content, just metadata).
    writer
        .add_entry("docs/", b"", 0o040755)
        .expect("failed to add docs/");
    writer
        .add_entry("src/", b"", 0o040755)
        .expect("failed to add src/");
    writer
        .add_entry("src/bin/", b"", 0o040700)
        .expect("failed to add src/bin/");

    // Regular files with different permissions.
    writer
        .add_entry("README.md", b"# Project\n\nA sample project.\n", 0o100644)
        .expect("failed to add README.md");
    writer
        .add_entry("docs/guide.txt", b"User guide content here.\n", 0o100644)
        .expect("failed to add docs/guide.txt");
    writer
        .add_entry(
            "src/main.rs",
            b"fn main() { println!(\"Hello\"); }\n",
            0o100644,
        )
        .expect("failed to add src/main.rs");

    // Executable files.
    writer
        .add_entry("build.sh", b"#!/bin/bash\ncargo build\n", 0o100755)
        .expect("failed to add build.sh");
    writer
        .add_entry("src/bin/tool", b"ELF binary placeholder", 0o100755)
        .expect("failed to add src/bin/tool");

    // Read-only file.
    writer
        .add_entry("LICENSE", b"MIT License\n", 0o100444)
        .expect("failed to add LICENSE");

    writer.sync().expect("failed to sync archive");
}

/// Generates a bale archive with 16KB alignment.
fn generate_align_16k_bale() {
    let fixtures_dir = Path::new(VALID_FIXTURES_DIR);
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
fn generate_path_2048_bale() {
    let fixtures_dir = Path::new(VALID_FIXTURES_DIR);
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

// =============================================================================
// Valid but not compacted fixtures
// =============================================================================

/// Generates a bale archive with orphaned data (gaps between entries).
///
/// Created by adding an entry, deleting it, then adding another entry.
/// The deleted entry's data remains as orphaned bytes.
fn generate_orphaned_data_bale() {
    let archive_path = Path::new(VALID_FIXTURES_DIR).join("orphaned_data.bale");

    // Remove existing archive if present.
    let _ = std::fs::remove_file(&archive_path);

    // Create archive with two entries, then delete the first one.
    let mut writer = ArchiveWriter::create(&archive_path).expect("failed to create archive");
    writer
        .add_entry("first.txt", b"This will be deleted", 0o644)
        .expect("failed to add first entry");
    writer
        .add_entry("second.txt", b"This stays", 0o644)
        .expect("failed to add second entry");
    writer.delete("first.txt");
    writer.sync().expect("failed to sync archive");
}

// =============================================================================
// Invalid (repairable) fixtures
// =============================================================================

/// Generates a bale archive with an unsorted directory table.
///
/// Entries are added in reverse alphabetical order (c, b, a) and the
/// directory table is not sorted, making binary search impossible.
fn generate_unsorted_cd_bale() {
    let archive_path = Path::new(INVALID_FIXTURES_DIR).join("unsorted_cd.bale");

    // Remove existing archive if present.
    let _ = std::fs::remove_file(&archive_path);

    // Create archive with entries in reverse order.
    // ArchiveWriter doesn't sort entries, so they stay in insertion order.
    let mut writer = ArchiveWriter::create(&archive_path).expect("failed to create archive");
    writer
        .add_entry("c.txt", b"third", 0o644)
        .expect("failed to add c.txt");
    writer
        .add_entry("b.txt", b"second", 0o644)
        .expect("failed to add b.txt");
    writer
        .add_entry("a.txt", b"first", 0o644)
        .expect("failed to add a.txt");
    writer.sync().expect("failed to sync archive");
}

/// Generates a bale archive with duplicate paths.
///
/// The same path appears multiple times in the directory table.
/// Only the last entry is accessible (shadowing).
fn generate_duplicate_paths_bale() {
    let archive_path = Path::new(INVALID_FIXTURES_DIR).join("duplicate_paths.bale");

    // Remove existing archive if present.
    let _ = std::fs::remove_file(&archive_path);

    // Create archive with duplicate paths.
    let mut writer = ArchiveWriter::create(&archive_path).expect("failed to create archive");
    writer
        .add_entry("file.txt", b"version 1", 0o644)
        .expect("failed to add first file.txt");
    writer
        .add_entry("file.txt", b"version 2", 0o644)
        .expect("failed to add second file.txt");
    writer
        .add_entry("file.txt", b"version 3", 0o644)
        .expect("failed to add third file.txt");
    writer.sync().expect("failed to sync archive");
}

/// Generates a bale archive with an incorrect CRC-32 checksum.
///
/// The archive is structurally valid but the CRC in the data block header
/// does not match the actual file data.
fn generate_bad_crc_bale() {
    use std::io::{Read, Seek, SeekFrom};

    let archive_path = Path::new(INVALID_FIXTURES_DIR).join("bad_crc.bale");

    // Remove existing archive if present.
    let _ = std::fs::remove_file(&archive_path);

    // Create a valid archive first.
    {
        let mut writer = ArchiveWriter::create(&archive_path).expect("failed to create archive");
        writer
            .add_entry("test.txt", b"test content", 0o644)
            .expect("failed to add entry");
        writer.sync().expect("failed to sync archive");
    }

    // Corrupt the CRC in the first data block header.
    // Data blocks start after the file header (8 bytes).
    // In a DataBlockHeader, crc32 is at offset 20 (entry_id:4 + file_size:8 + block_size:8).
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&archive_path)
        .expect("failed to open archive");

    let crc_offset = FileHeader::SIZE as u64 + 20;

    // Read current CRC.
    file.seek(SeekFrom::Start(crc_offset))
        .expect("failed to seek");
    let mut crc_bytes = [0u8; 4];
    file.read_exact(&mut crc_bytes).expect("failed to read CRC");

    // Corrupt it by flipping bits.
    crc_bytes[0] ^= 0xFF;

    // Write corrupted CRC back.
    file.seek(SeekFrom::Start(crc_offset))
        .expect("failed to seek");
    file.write_all(&crc_bytes)
        .expect("failed to write corrupted CRC");
}
