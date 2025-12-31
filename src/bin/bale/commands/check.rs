//! Check command implementation.

use std::path::Path;

use bale::{ArchiveReader, compact, rename_duplicates, repair_crcs};

use crate::error::BaleCliError;

/// Archive status after checking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveStatus {
    /// Archive is compacted (sorted, no duplicates, no orphaned data).
    Compacted,
    /// Archive is working but not compacted.
    Working,
}

/// Classification of CRC issues.
#[derive(Debug)]
enum CrcIssue {
    /// CD CRC wrong, but Local matches computed - safe to fix with --fix.
    CdOnly {
        /// The file path with the issue.
        path: String,
    },
    /// Local CRC wrong, but CD matches computed - safe to fix with --fix.
    LocalOnly {
        /// The file path with the issue.
        path: String,
    },
    /// Both CRCs wrong - needs --fix-crc.
    Both {
        /// The file path with the issue.
        path: String,
    },
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
/// When `fix` is true, attempts to fix safe issues:
/// - Unsorted CD: Sorts by running compact
/// - Duplicate paths: Renames with numeric suffixes (e.g., `file(1).txt`)
/// - CRC mismatches where one header is correct: Copies correct value
///
/// When `fix_crc` is true, also fixes risky issues:
/// - CRC mismatches where both headers are wrong: Recomputes from data
///
/// # Errors
///
/// Returns an error if the archive cannot be opened or read.
pub fn run(archive_path: impl AsRef<Path>, fix: bool, fix_crc: bool) -> Result<(), BaleCliError> {
    let mut errors: Vec<String> = Vec::new();
    let mut fixed: Vec<String> = Vec::new();

    // First pass: check CRCs and classify issues.
    let (crc_issues, is_sorted) = {
        let reader = ArchiveReader::open(&archive_path)?;
        let mut crc_issues: Vec<CrcIssue> = Vec::new();

        for (header, path_bytes) in reader.iter_entries() {
            let path = path_to_string(path_bytes);
            let (computed, local_crc, cd_crc) = reader.crc_info(header)?;

            if computed == local_crc && computed == cd_crc {
                // All good.
            } else if computed == local_crc && computed != cd_crc {
                // CD is wrong, Local is correct.
                crc_issues.push(CrcIssue::CdOnly { path });
            } else if computed == cd_crc && computed != local_crc {
                // Local is wrong, CD is correct.
                crc_issues.push(CrcIssue::LocalOnly { path });
            } else {
                // Both are wrong.
                crc_issues.push(CrcIssue::Both { path });
            }
        }

        let is_sorted = reader.is_sorted();
        (crc_issues, is_sorted)
    };

    // Classify CRC issues into safe and risky.
    let mut safe_crc_issues: Vec<String> = Vec::new();
    let mut risky_crc_issues: Vec<String> = Vec::new();

    for issue in &crc_issues {
        match issue {
            CrcIssue::CdOnly { path } | CrcIssue::LocalOnly { path } => {
                safe_crc_issues.push(path.clone());
            }
            CrcIssue::Both { path } => {
                risky_crc_issues.push(path.clone());
            }
        }
    }

    // Handle CRC fixes.
    let has_crc_issues = !crc_issues.is_empty();
    let crc_fixed = if has_crc_issues && (fix || fix_crc) {
        // Both --fix and --fix-crc trigger repair (repair_crcs fixes all CRC issues).
        // But we only report as "fixed" based on what flags were used.
        let can_fix_safe = fix || fix_crc;
        let can_fix_risky = fix_crc;

        let should_repair = (can_fix_safe && !safe_crc_issues.is_empty())
            || (can_fix_risky && !risky_crc_issues.is_empty());

        if should_repair {
            let stats = repair_crcs(&archive_path)?;
            for path in &stats.repaired {
                fixed.push(format!("Repaired CRC for '{}'", path));
            }
            true
        } else {
            false
        }
    } else {
        false
    };

    // Report CRC errors that weren't fixed.
    if !crc_fixed {
        for issue in &crc_issues {
            match issue {
                CrcIssue::CdOnly { path } => {
                    errors.push(format!(
                        "'{}': corrupted archive: CD CRC mismatch (use --fix to repair)",
                        path
                    ));
                }
                CrcIssue::LocalOnly { path } => {
                    errors.push(format!(
                        "'{}': corrupted archive: Local header CRC mismatch (use --fix to repair)",
                        path
                    ));
                }
                CrcIssue::Both { path } => {
                    errors.push(format!(
                        "'{}': corrupted archive: CRC mismatch (use --fix-crc to repair)",
                        path
                    ));
                }
            }
        }
    }

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

    // Second pass: check duplicates and fix if requested.
    let has_duplicates = {
        let reader = ArchiveReader::open(&archive_path)?;
        let duplicates = reader.find_duplicates();
        let has_duplicates = !duplicates.is_empty();

        if has_duplicates && fix {
            drop(reader); // Release the reader before modifying.
            let stats = rename_duplicates(&archive_path)?;
            for (old_path, new_path) in &stats.renames {
                fixed.push(format!("Renamed '{}' to '{}'", old_path, new_path));
            }
            false // No longer has duplicates after fix.
        } else {
            for path in &duplicates {
                errors.push(format!("Duplicate path: '{}'", path));
            }
            has_duplicates
        }
    };

    // Third pass: check orphaned data.
    let has_orphaned_data = {
        let reader = ArchiveReader::open(&archive_path)?;
        let has_orphaned_data = reader.has_orphaned_data();
        if has_orphaned_data {
            errors
                .push("Archive contains orphaned data (run 'bale compact' to reclaim)".to_string());
        }
        has_orphaned_data
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
    let has_crc_errors = !crc_fixed && has_crc_issues;
    let status = if is_sorted && !has_duplicates && !has_orphaned_data && !has_crc_errors {
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
