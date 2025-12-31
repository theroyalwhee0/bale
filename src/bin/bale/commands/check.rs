//! Check command implementation.

use std::path::Path;

use bale::{ArchiveReader, compact};

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
/// When `fix` is true, attempts to fix issues:
/// - Unsorted CD: Sorts by running compact
///
/// # Errors
///
/// Returns an error if the archive cannot be opened or read.
pub fn run(archive_path: impl AsRef<Path>, fix: bool) -> Result<(), BaleCliError> {
    let mut errors: Vec<String> = Vec::new();
    let mut fixed: Vec<String> = Vec::new();

    // First pass: check CRCs and sorting.
    let (crc_errors, is_sorted) = {
        let reader = ArchiveReader::open(&archive_path)?;
        let mut crc_errors = Vec::new();

        for (header, path_bytes) in reader.iter_entries() {
            if let Err(e) = reader.verify_crc(header) {
                let path = path_to_string(path_bytes);
                crc_errors.push(format!("'{}': {}", path, e));
            }
        }

        let is_sorted = reader.is_sorted();
        (crc_errors, is_sorted)
    };

    errors.extend(crc_errors);

    // Fix unsorted CD if requested.
    let is_sorted = if !is_sorted && fix {
        compact(&archive_path)?;
        fixed.push("Sorted Central Directory".to_string());
        true
    } else {
        if !is_sorted {
            errors.push("Central Directory is not sorted by path".to_string());
        }
        is_sorted
    };

    // Second pass: check duplicates and orphaned data.
    let (has_duplicates, has_orphaned_data) = {
        let reader = ArchiveReader::open(&archive_path)?;

        let duplicates = reader.find_duplicates();
        let has_duplicates = !duplicates.is_empty();
        for path in &duplicates {
            errors.push(format!("Duplicate path: '{}'", path));
        }

        let has_orphaned_data = reader.has_orphaned_data();
        if has_orphaned_data {
            errors
                .push("Archive contains orphaned data (run 'bale compact' to reclaim)".to_string());
        }

        (has_duplicates, has_orphaned_data)
    };

    // Print fixed items to stdout.
    #[allow(clippy::print_stdout)]
    for item in &fixed {
        println!("fixed: {item}");
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

/// Converts a null-padded path to a string.
fn path_to_string(path_bytes: &[u8]) -> String {
    let end = path_bytes
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(path_bytes.len());
    String::from_utf8_lossy(&path_bytes[..end]).to_string()
}
