//! List command implementation.

use std::path::Path;

use bale::{ArchivePath, ArchiveReader};

use crate::error::BaleCliError;

/// Lists entries in an archive.
pub fn run(archive_path: impl AsRef<Path>) -> Result<(), BaleCliError> {
    let reader = ArchiveReader::open(archive_path)?;

    for (header, path_bytes) in reader.iter_entries() {
        let path = ArchivePath::from_null_padded_bytes(path_bytes);
        let size = header.uncompressed_size.get();
        let mode = header.external_attrs.get() >> 16;

        #[allow(clippy::print_stdout)]
        {
            println!("{mode:06o} {size:>10} {path}");
        }
    }

    Ok(())
}
