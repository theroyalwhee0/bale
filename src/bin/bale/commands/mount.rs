//! Mount a bale archive as a FUSE filesystem.

use std::path::PathBuf;

use crate::error::BaleCliError;

/// Runs the mount command.
///
/// # Errors
///
/// Returns an error if:
/// - The archive cannot be opened
/// - The mount point is invalid
/// - FUSE mounting fails
pub fn run(
    _archive: PathBuf,
    _mount_point: PathBuf,
    _background: bool,
    _allow_root: bool,
    _allow_other: bool,
    _shell: Option<Option<String>>,
) -> Result<(), BaleCliError> {
    // TODO: Implement FUSE filesystem mounting
    // 1. Open archive with ArchiveReader
    // 2. Create BaleFs instance
    // 3. If shell is Some, spawn shell after mount
    // 4. Mount filesystem (foreground or background)
    Err(BaleCliError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "FUSE mount not yet implemented",
    )))
}
