//! Add command implementation.

use std::path::{Path, PathBuf};

use bale::Archive;

use crate::error::BaleCliError;

/// Adds files to an archive.
pub fn run(archive_path: impl AsRef<Path>, files: &[PathBuf]) -> Result<(), BaleCliError> {
    let mut archive = Archive::create(archive_path)?;

    for file in files {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed");
        archive.add_file(file, name)?;
    }

    archive.finish()?;
    Ok(())
}
