//! Add command implementation.

use std::path::{Path, PathBuf};

use crate::error::BaleCliError;

/// Adds files to an archive.
///
/// Calls [`bale::add`] and prints each added path plus a total count.
/// The archive must already exist.
///
/// # Errors
///
/// Returns an error if adding files fails.
pub fn run(
    archive_path: impl AsRef<Path>,
    prefix: &str,
    files: &[PathBuf],
) -> Result<(), BaleCliError> {
    let added = bale::add(archive_path, prefix, files)?;

    #[allow(clippy::print_stdout)]
    {
        for path in &added {
            println!("  added: {path}");
        }
        println!("Added {} entries", added.len());
    }

    Ok(())
}
