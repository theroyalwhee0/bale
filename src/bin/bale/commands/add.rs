//! Add command implementation.

use std::path::{Path, PathBuf};

use bale::{ArchivePath, ArchiveWriter};

use crate::error::BaleCliError;

/// Adds files to an archive.
pub fn run(
    archive_path: impl AsRef<Path>,
    prefix: &str,
    files: &[PathBuf],
) -> Result<(), BaleCliError> {
    let mut writer = ArchiveWriter::create(archive_path)?;

    for file in files {
        let name = file.file_name().unwrap_or(file.as_os_str());
        let dest = Path::new(prefix).join(name);
        let archive_path = ArchivePath::try_from_path(&dest)?;
        writer.add_file(file, archive_path.as_str())?;
    }

    writer.sync()?;
    Ok(())
}
