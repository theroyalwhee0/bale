//! Delete command implementation.

use std::path::Path;

use bale::ArchiveWriter;

use crate::error::BaleCliError;

/// Deletes entries from an archive.
pub fn run(archive_path: impl AsRef<Path>, entries: &[String]) -> Result<(), BaleCliError> {
    let mut writer = ArchiveWriter::open(archive_path)?;

    let mut deleted_count = 0usize;
    for entry_path in entries {
        if writer.delete(entry_path) {
            deleted_count += 1;
            #[allow(clippy::print_stdout)]
            {
                println!("  deleted: {entry_path}");
            }
        } else {
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

    Ok(())
}
