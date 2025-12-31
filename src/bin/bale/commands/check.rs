//! Check command implementation.

use std::collections::HashSet;
use std::path::Path;

use bale::{ArchiveReader, CentralDirectoryHeader, LocalFileHeader};

use crate::error::BaleCliError;

/// Archive status after checking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveStatus {
    /// Archive is compacted (sorted, no duplicates, no orphaned data).
    Compacted,
    /// Archive is working but not compacted.
    Working,
}

/// Checks archive integrity.
///
/// Verifies:
/// - Format validity (via `ArchiveReader::open`)
/// - CRC-32 checksums
/// - Central Directory ordering (sorted by path)
/// - Duplicate path detection
/// - Orphaned data detection
///
/// # Errors
///
/// Returns an error if the archive cannot be opened or read.
pub fn run(archive_path: impl AsRef<Path>) -> Result<(), BaleCliError> {
    let reader = ArchiveReader::open(&archive_path)?;

    let mut errors: Vec<String> = Vec::new();

    // Collect entries for analysis.
    let entries: Vec<(&CentralDirectoryHeader, &[u8])> = reader.iter_entries().collect();

    // Check CRCs.
    check_crcs(&reader, &entries, &mut errors);

    // Check if CD is sorted by path bytes.
    let is_sorted = check_sorted(&entries);
    if !is_sorted {
        errors.push("Central Directory is not sorted by path".to_string());
    }

    // Check for duplicate paths.
    let has_duplicates = check_duplicates(&entries, &mut errors);

    // Check for orphaned data (gaps between entries).
    let has_orphaned_data = check_orphaned_data(&reader, &entries);
    if has_orphaned_data {
        errors.push("Archive contains orphaned data (run 'bale compact' to reclaim)".to_string());
    }

    // Print errors to stderr.
    #[allow(clippy::print_stderr)]
    for error in &errors {
        eprintln!("error: {error}");
    }

    // Determine status and print result.
    let status = if is_sorted && !has_duplicates && !has_orphaned_data {
        ArchiveStatus::Compacted
    } else {
        ArchiveStatus::Working
    };

    #[allow(clippy::print_stdout)]
    match status {
        ArchiveStatus::Compacted => println!("Success (Compacted)"),
        ArchiveStatus::Working => println!("Success (Working)"),
    }

    Ok(())
}

/// Verifies CRC-32 checksums for all entries.
fn check_crcs(
    reader: &ArchiveReader,
    entries: &[(&CentralDirectoryHeader, &[u8])],
    errors: &mut Vec<String>,
) {
    for (header, path_bytes) in entries {
        let path = path_to_string(path_bytes);

        // Read data and compute CRC.
        match reader.read_data(header) {
            Ok(data) => {
                let computed_crc = crc32fast::hash(data);
                let stored_crc = header.crc32.get();

                if computed_crc != stored_crc {
                    errors.push(format!(
                        "CRC mismatch for '{}': expected {:08x}, got {:08x}",
                        path, stored_crc, computed_crc
                    ));
                }
            }
            Err(e) => {
                errors.push(format!("Failed to read data for '{}': {}", path, e));
            }
        }
    }
}

/// Checks if the Central Directory is sorted by path bytes.
fn check_sorted(entries: &[(&CentralDirectoryHeader, &[u8])]) -> bool {
    entries.windows(2).all(|w| w[0].1 <= w[1].1)
}

/// Checks for duplicate paths in the archive.
///
/// Returns `true` if duplicates were found.
fn check_duplicates(
    entries: &[(&CentralDirectoryHeader, &[u8])],
    errors: &mut Vec<String>,
) -> bool {
    let mut seen: HashSet<&[u8]> = HashSet::new();
    let mut has_duplicates = false;

    for (_header, path_bytes) in entries {
        if !seen.insert(*path_bytes) {
            let path = path_to_string(path_bytes);
            errors.push(format!("Duplicate path: '{}'", path));
            has_duplicates = true;
        }
    }

    has_duplicates
}

/// Checks for orphaned data (gaps between entries or before CD).
///
/// Returns `true` if orphaned data was detected.
fn check_orphaned_data(
    reader: &ArchiveReader,
    entries: &[(&CentralDirectoryHeader, &[u8])],
) -> bool {
    if entries.is_empty() {
        return false;
    }

    let path_size = reader.path_size();
    let alignment = reader.alignment() as usize;
    let local_header_stride = LocalFileHeader::stride(path_size);
    let cd_offset = reader.eocd().cd_offset.get() as usize;

    // Calculate expected end position after all entries.
    let mut expected_offset: usize = 0;

    for (header, _path_bytes) in entries {
        let local_offset = header.local_header_offset.get() as usize;
        let data_size = header.uncompressed_size.get() as usize;

        // Check if entry starts where expected.
        if local_offset != expected_offset {
            return true;
        }

        // Calculate next expected offset (aligned).
        let entry_size = local_header_stride + data_size;
        let aligned_size = entry_size.div_ceil(alignment) * alignment;
        expected_offset = local_offset + aligned_size;
    }

    // Check if CD starts right after the last entry.
    expected_offset != cd_offset
}

/// Converts a null-padded path to a string.
fn path_to_string(path_bytes: &[u8]) -> String {
    let end = path_bytes
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(path_bytes.len());
    String::from_utf8_lossy(&path_bytes[..end]).to_string()
}
