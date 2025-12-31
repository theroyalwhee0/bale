//! Fixture generation tests.
//!
//! Run with `cargo test --test fixtures -- --ignored` to regenerate fixtures.

use std::fs::File;
use std::io::Write;
use std::path::Path;

use bale::{ArchiveWriter, BaleEocd, Eocd};
use zerocopy::IntoBytes;

const VALID_FIXTURES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/valid");
const INVALID_FIXTURES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/invalid");

/// Generates an empty bale archive with EOCD + BaleEocd (256 bytes).
#[test]
#[ignore]
fn generate_empty_bale() {
    let path = Path::new(VALID_FIXTURES_DIR).join("empty.bale");
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

/// Generates a bale archive with 16KB alignment.
#[test]
#[ignore]
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
#[test]
#[ignore]
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
#[test]
#[ignore]
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

/// Generates a bale archive with an unsorted Central Directory.
///
/// Entries are added in reverse alphabetical order (c, b, a) and the CD
/// is not sorted, making binary search impossible.
#[test]
#[ignore]
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
/// The same path appears multiple times in the Central Directory.
/// Only the last entry is accessible (shadowing).
#[test]
#[ignore]
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
/// The archive is structurally valid but the CRC in the Central Directory
/// does not match the actual file data.
#[test]
#[ignore]
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

    // Now corrupt the CRC in the Central Directory.
    // The CD is located before the trailer (last 256 bytes).
    // CRC32 is at offset 16 in the CentralDirectoryHeader.
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&archive_path)
        .expect("failed to open archive");

    let file_len = file.metadata().expect("failed to get metadata").len();
    let trailer_size = 256u64; // EOCD (22) + BaleEocd (234)
    let cd_header_size = 46u64;
    let path_size = 256u64;
    let cd_entry_size = cd_header_size + path_size;

    // CD starts at file_len - trailer_size - cd_entry_size
    let cd_offset = file_len - trailer_size - cd_entry_size;

    // CRC32 is at offset 16 in the CD header.
    let crc_offset = cd_offset + 16;

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
