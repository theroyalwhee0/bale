//! Check command implementation.

use std::path::Path;

use crate::error::BaleCliError;

/// Checks archive integrity.
///
/// Calls [`bale::check`] and formats the output. When `quiet` is true,
/// suppresses all output (use exit code only).
///
/// # Errors
///
/// Returns an error if:
/// - The archive cannot be opened or read
/// - Any integrity issues are found (CRC errors, duplicates, etc.)
pub fn run(archive_path: impl AsRef<Path>, quiet: bool) -> Result<(), BaleCliError> {
    let report = bale::check(&archive_path)?;

    if !quiet {
        // Print errors to stderr.
        #[allow(clippy::print_stderr)]
        for issue in &report.issues {
            eprintln!("error: {issue}");
        }

        // Print status to stdout.
        #[allow(clippy::print_stdout)]
        if report.is_compacted {
            println!("Status: Compacted");
        } else {
            println!("Status: Working");
        }
    }

    // Return error if any issues were found.
    if report.issues.is_empty() {
        Ok(())
    } else {
        Err(BaleCliError::CheckFailed(report.issues.len()))
    }
}
