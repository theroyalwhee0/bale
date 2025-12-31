//! Check command implementation.

use std::path::Path;

use bale::ArchiveReader;

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

    // Check CRCs for all entries.
    for (header, path_bytes) in reader.iter_entries() {
        if let Err(e) = reader.verify_crc(header) {
            let path = path_to_string(path_bytes);
            errors.push(format!("'{}': {}", path, e));
        }
    }

    // Check if CD is sorted.
    let is_sorted = reader.is_sorted();
    if !is_sorted {
        errors.push("Central Directory is not sorted by path".to_string());
    }

    // Check for duplicate paths.
    let duplicates = reader.find_duplicates();
    let has_duplicates = !duplicates.is_empty();
    for path in &duplicates {
        errors.push(format!("Duplicate path: '{}'", path));
    }

    // Check for orphaned data.
    let has_orphaned_data = reader.has_orphaned_data();
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

/// Converts a null-padded path to a string.
fn path_to_string(path_bytes: &[u8]) -> String {
    let end = path_bytes
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(path_bytes.len());
    String::from_utf8_lossy(&path_bytes[..end]).to_string()
}
