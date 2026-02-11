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
///
/// This writes raw bytes to bypass `ArchiveWriter::sync()` which sorts
/// the directory table.
fn generate_unsorted_cd_bale() {
    use bale::{Crc, DirectoryRow, EntryRow};
    use zerocopy::byteorder::little_endian::{U16, U32, U64};

    let archive_path = Path::new(INVALID_FIXTURES_DIR).join("unsorted_cd.bale");

    // Remove existing archive if present.
    let _ = std::fs::remove_file(&archive_path);

    // Files in deliberately unsorted order (c, b, a).
    let files: &[(&str, &[u8], u32)] = &[
        ("c.txt", b"third", 1),
        ("b.txt", b"second", 2),
        ("a.txt", b"first", 3),
    ];

    let alignment: u32 = 4096;
    let path_size: u16 = 256;
    let mut buf = Vec::new();

    // 1. File header (8 bytes).
    let file_header = FileHeader::new();
    buf.extend_from_slice(file_header.as_bytes());

    // 2. Data blocks (aligned, raw bytes).
    struct DataInfo {
        /// Offset in the archive.
        offset: u64,
        /// CRC-32C of the data.
        crc: Crc,
    }
    let mut data_infos = Vec::new();
    for &(_, data, _) in files {
        let current = buf.len();
        let padding = (alignment as usize - (current % alignment as usize)) % alignment as usize;
        buf.extend(std::iter::repeat_n(0u8, padding));
        let data_offset = buf.len() as u64;
        let crc = Crc::compute(data);
        buf.extend_from_slice(data);
        data_infos.push(DataInfo {
            offset: data_offset,
            crc,
        });
    }

    // 3. Entry table (sorted by entry ID).
    let mut entry_rows: Vec<(u32, EntryRow)> = files
        .iter()
        .zip(data_infos.iter())
        .map(|(&(_, data, id), di)| {
            let row = EntryRow::new_file(
                id,
                di.crc,
                di.offset,
                data.len() as u64,
                data.len() as u64,
                1_700_000_000_000,
                1_700_000_001_000,
                0o100644,
            );
            (id, row)
        })
        .collect();
    entry_rows.sort_by_key(|(id, _)| *id);

    let entry_table_offset = buf.len() as u64;
    for (_, row) in &entry_rows {
        buf.extend_from_slice(row.as_bytes());
    }
    let entry_count = entry_rows.len() as u32;

    // 4. Directory table (deliberately UNSORTED: c, b, a).
    let directory_table_offset = buf.len() as u64;
    let stride = DirectoryRow::stride(path_size);
    for &(path, _, id) in files {
        let mut row_bytes = vec![0u8; stride];
        let path_bytes = path.as_bytes();
        row_bytes[..path_bytes.len()].copy_from_slice(path_bytes);
        let id_offset = path_size as usize;
        row_bytes[id_offset..id_offset + 4].copy_from_slice(&id.to_le_bytes());
        buf.extend_from_slice(&row_bytes);
    }
    let directory_entry_count = files.len() as u32;

    // 5. Trailer (64 bytes).
    let next_id = files.iter().map(|&(_, _, id)| id).max().unwrap_or(0) + 1;
    let mut trailer = Trailer::new();
    trailer.entry_table_offset = U64::new(entry_table_offset);
    trailer.entry_count = U32::new(entry_count);
    trailer.directory_table_offset = U64::new(directory_table_offset);
    trailer.directory_entry_count = U32::new(directory_entry_count);
    trailer.next_id = U32::new(next_id);
    trailer.alignment_power = alignment.trailing_zeros() as u8;
    trailer.path_size = U16::new(path_size);
    buf.extend_from_slice(trailer.as_bytes());

    // Write to file.
    let mut file = File::create(&archive_path).expect("failed to create unsorted_cd.bale");
    file.write_all(&buf)
        .expect("failed to write unsorted_cd.bale");
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

/// Generates a bale archive with an incorrect CRC-32C checksum.
///
/// The archive is structurally valid but the CRC-32C in the entry row
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

    // Corrupt the CRC-32C in the first entry row.
    // The entry table offset is read from the trailer. The CRC-32C field
    // is at offset 4 within the 64-byte entry row (after the 4-byte entry_id).
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&archive_path)
        .expect("failed to open archive");

    // Read entry_table_offset from the trailer (at file_len - 64 + 8).
    let file_len = file.metadata().expect("failed to stat").len();
    file.seek(SeekFrom::Start(file_len - Trailer::SIZE as u64 + 8))
        .expect("failed to seek to trailer");
    let mut offset_bytes = [0u8; 8];
    file.read_exact(&mut offset_bytes)
        .expect("failed to read entry_table_offset");
    let entry_table_offset = u64::from_le_bytes(offset_bytes);

    // CRC-32C is at entry_table_offset + 4.
    let crc_offset = entry_table_offset + 4;

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
