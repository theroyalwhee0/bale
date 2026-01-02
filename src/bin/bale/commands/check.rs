//! Check command implementation.

use std::path::Path;

use bale::{ArchivePath, ArchiveReader};

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
/// When `quiet` is true, suppresses all output (use exit code only).
///
/// # Errors
///
/// Returns an error if:
/// - The archive cannot be opened or read
/// - Any integrity issues are found (CRC errors, duplicates, etc.)
pub fn run(archive_path: impl AsRef<Path>, quiet: bool) -> Result<(), BaleCliError> {
    let reader = ArchiveReader::open(&archive_path)?;
    let mut errors: Vec<String> = Vec::new();

    // Check CRCs.
    for (header, path_bytes) in reader.iter_entries() {
        if let Err(e) = reader.verify_crc(header) {
            let path = ArchivePath::from_null_padded_bytes(path_bytes);
            errors.push(format!("'{}': {}", path, e));
        }
    }

    // Check sorting.
    let is_sorted = reader.is_sorted();
    if !is_sorted {
        errors.push("Central Directory is not sorted by path".to_string());
    }

    // Check duplicates.
    let duplicates = reader.find_duplicates();
    let has_duplicates = !duplicates.is_empty();
    for path in &duplicates {
        errors.push(format!("Duplicate path: '{}'", path));
    }

    // Check orphaned data.
    let has_orphaned_data = reader.has_orphaned_data();
    if has_orphaned_data {
        errors.push("Archive contains orphaned data (run 'bale compact' to reclaim)".to_string());
    }

    if !quiet {
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
            ArchiveStatus::Compacted => println!("Status: Compacted"),
            ArchiveStatus::Working => println!("Status: Working"),
        }
    }

    // Return error if any issues were found.
    if errors.is_empty() {
        Ok(())
    } else {
        Err(BaleCliError::CheckFailed(errors.len()))
    }
}
