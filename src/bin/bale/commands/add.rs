//! Add command implementation.

use std::path::{Path, PathBuf};

use bale::{ArchivePath, ArchiveWrite, ArchiveWriter};

use crate::error::BaleCliError;

/// Adds files to an archive.
///
/// The archive must already exist. Use `bale touch` to create a new archive.
pub fn run(
    archive_path: impl AsRef<Path>,
    prefix: &str,
    files: &[PathBuf],
) -> Result<(), BaleCliError> {
    let mut writer = ArchiveWriter::open(archive_path)?;

    let mut added_count = 0usize;
    for file in files {
        let name = file.file_name().unwrap_or(file.as_os_str());
        let dest = Path::new(prefix).join(name);
        let entry_path = ArchivePath::try_from(dest)?;
        let entry_str = entry_path.as_str().expect("path validated as UTF-8");

        writer.add_file(file, entry_str)?;
        added_count += 1;

        #[allow(clippy::print_stdout)]
        {
            println!("  added: {entry_str}");
        }
    }

    writer.sync()?;

    #[allow(clippy::print_stdout)]
    {
        println!("Added {added_count} entries");
    }

    Ok(())
}
