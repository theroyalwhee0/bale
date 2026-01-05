//! Mount a bale archive as a FUSE filesystem.

use std::path::PathBuf;

use bale::fuse::BaleFs;

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
    archive: PathBuf,
    mount_point: PathBuf,
    background: bool,
    allow_root: bool,
    allow_other: bool,
    _shell: Option<Option<String>>,
) -> Result<(), BaleCliError> {
    // TODO: Implement --background (daemonize)
    // TODO: Implement --shell (spawn shell at mount point)
    if background {
        return Err(BaleCliError::Io(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "--background not yet implemented",
        )));
    }

    // For now, always mount read-only.
    let read_only = true;

    let fs = BaleFs::new(&archive, read_only)?;
    fs.mount(&mount_point, allow_root, allow_other)?;

    Ok(())
}
