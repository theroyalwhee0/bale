//! List command implementation.

use std::path::Path;

use bale::ArchiveReader;

use crate::error::BaleCliError;

/// Lists entries in an archive.
pub fn run(archive_path: impl AsRef<Path>) -> Result<(), BaleCliError> {
    let reader = ArchiveReader::open(archive_path)?;

    #[allow(clippy::print_stdout)]
    for (header, path_bytes) in reader.iter_entries() {
        // Convert path bytes to string, trimming null padding.
        let path: String = path_bytes
            .iter()
            .copied()
            .take_while(|&b| b != 0)
            .map(|b| b as char)
            .collect();

        let size = header.uncompressed_size.get();
        let mode = header.external_attrs.get() >> 16;

        println!("{mode:06o} {size:>10} {path}");
    }

    Ok(())
}
