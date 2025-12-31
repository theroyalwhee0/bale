//! Compact command implementation.

use std::path::Path;

use bale::compact;

use crate::error::BaleCliError;

/// Compacts an archive, removing orphaned data.
pub fn run(archive_path: impl AsRef<Path>) -> Result<(), BaleCliError> {
    let stats = compact(archive_path)?;

    #[allow(clippy::print_stdout)]
    {
        println!("Compacted archive:");
        println!("  Original size:    {} bytes", stats.original_size);
        println!("  Compacted size:   {} bytes", stats.compacted_size);
        println!("  Entries removed:  {}", stats.entries_removed);
        println!("  Bytes reclaimed:  {}", stats.bytes_reclaimed);
    }

    Ok(())
}
