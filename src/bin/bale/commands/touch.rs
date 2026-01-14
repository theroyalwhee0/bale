//! Touch command implementation.

use std::fs::{FileTimes, OpenOptions};
use std::path::Path;
use std::time::SystemTime;

use bale::{ArchiveReader, ArchiveWrite, ArchiveWriter};

use crate::error::BaleCliError;

/// Creates a bale archive or updates its modification time.
///
/// If the file doesn't exist, creates a new empty bale archive with the
/// specified path size. If the file exists, validates it is a valid bale
/// archive and then updates its modification time (path_size is ignored).
/// Returns an error if the file exists but is not a valid bale archive.
pub fn run(path: impl AsRef<Path>, path_size: u16) -> Result<(), BaleCliError> {
    let path = path.as_ref();

    if path.exists() {
        // Validate the file is a valid bale archive before touching.
        ArchiveReader::open(path)?;

        let file = OpenOptions::new().write(true).open(path)?;
        let now = SystemTime::now();
        let times = FileTimes::new().set_accessed(now).set_modified(now);
        file.set_times(times)?;
    } else {
        let mut writer = ArchiveWriter::create_with_options(path, 4096, path_size)?;
        writer.sync()?;
    }

    Ok(())
}
