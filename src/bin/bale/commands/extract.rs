//! Extract command implementation.

use std::path::Path;

use crate::error::BaleCliError;

/// Extracts entries from an archive.
///
/// Calls [`bale::extract`] and prints each extracted path. If no entries
/// are specified, extracts all entries.
///
/// # Errors
///
/// Returns an error if extraction fails.
pub fn run(
    archive_path: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
    entries: &[String],
) -> Result<(), BaleCliError> {
    let extracted = bale::extract(archive_path, output_dir, entries)?;

    #[allow(clippy::print_stdout)]
    for path in &extracted {
        println!("  extracted: {path}");
    }

    Ok(())
}
