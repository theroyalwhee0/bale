//! Add command implementation.

use std::path::{Path, PathBuf};

use bale::Archive;

use crate::error::BaleCliError;

/// Adds files to an archive.
pub fn run(
    archive_path: impl AsRef<Path>,
    prefix: &str,
    files: &[PathBuf],
) -> Result<(), BaleCliError> {
    let mut archive = Archive::create(archive_path)?;

    for file in files {
        let name = file.file_name().unwrap_or(file.as_os_str());
        let dest = Path::new(prefix).join(name);
        archive.add_file_path(file, &dest)?;
    }

    archive.finish()?;
    Ok(())
}
