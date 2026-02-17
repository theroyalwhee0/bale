//! Delete command implementation.

use std::path::Path;

use bale::{ArchiveWrite, ArchiveWriter, BaleError};

use crate::error::BaleCliError;

/// Deletes entries from an archive.
///
/// Returns an error if any requested entries are missing, unless `ignore_missing` is true.
pub fn run(
    archive_path: impl AsRef<Path>,
    entries: &[String],
    ignore_missing: bool,
) -> Result<(), BaleCliError> {
    let mut writer = ArchiveWriter::open(archive_path)?;

    let mut deleted_count = 0usize;
    let mut missing: Vec<&str> = Vec::new();

    for entry_path in entries {
        if writer.delete(entry_path) {
            deleted_count += 1;
            #[allow(clippy::print_stdout)]
            {
                println!("  deleted: {entry_path}");
            }
        } else {
            missing.push(entry_path);
            #[allow(clippy::print_stdout)]
            {
                println!("  not found: {entry_path}");
            }
        }
    }

    writer.sync()?;

    #[allow(clippy::print_stdout)]
    {
        println!("Deleted {deleted_count} entries");
    }

    // Error if any entries were missing (unless --ignore-missing).
    if !missing.is_empty() && !ignore_missing {
        return Err(BaleError::EntryNotFound(missing.join(", ")).into());
    }

    Ok(())
}
