//! Touch command implementation.

use std::fs::{FileTimes, OpenOptions};
use std::path::Path;
use std::time::SystemTime;

use bale::Archive;

use crate::error::BaleCliError;

/// Creates a bale archive or updates its modification time.
///
/// If the file doesn't exist, creates a new empty bale archive.
/// If the file exists, updates its modification time.
pub fn run(path: impl AsRef<Path>) -> Result<(), BaleCliError> {
    let path = path.as_ref();

    if path.exists() {
        let file = OpenOptions::new().write(true).open(path)?;
        let now = SystemTime::now();
        let times = FileTimes::new().set_accessed(now).set_modified(now);
        file.set_times(times)?;
    } else {
        Archive::create(path)?.finish()?;
    }

    Ok(())
}
