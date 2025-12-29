//! Bale archive format library.
//!
//! A mmap-first, zero-copy zip-compatible archive format with fixed-stride
//! entries for efficient random access.

/// End of Central Directory record.
mod eocd;
/// Error types for bale operations.
mod error;

pub use eocd::Eocd;
pub use error::BaleError;

use std::fs::{FileTimes, OpenOptions};
use std::io;
use std::path::Path;
use std::time::SystemTime;

use memmap2::MmapMut;
use zerocopy::IntoBytes;

/// Default alignment for file data placement.
pub const ALIGNMENT: usize = 4096;

/// Default maximum path size in bytes.
pub const PATH_SIZE: usize = 256;

/// Creates a file or updates its modification time.
///
/// If the path has a `.bale` extension and the file doesn't exist,
/// creates a new empty bale archive. Otherwise, creates an empty file
/// or updates the modification time of an existing file.
///
/// # Errors
///
/// Returns an error if file creation or modification fails.
pub fn touch(path: &Path) -> Result<(), BaleError> {
    let is_bale = path.extension().is_some_and(|ext| ext == "bale");
    let exists = path.exists();

    if is_bale && !exists {
        create_empty_archive(path)
    } else {
        touch_file(path).map_err(BaleError::from)
    }
}

/// Creates a new empty bale archive at the given path.
///
/// The archive contains only the End of Central Directory record.
#[allow(unsafe_code)]
fn create_empty_archive(path: &Path) -> Result<(), BaleError> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)?;

    file.set_len(Eocd::SIZE as u64)?;

    // SAFETY: We just created this file exclusively with create_new.
    let mut mmap = unsafe { MmapMut::map_mut(&file)? };

    let eocd = Eocd::empty();
    mmap.copy_from_slice(eocd.as_bytes());
    mmap.flush()?;

    Ok(())
}

/// Updates the modification time of a file, creating it if it doesn't exist.
fn touch_file(path: &Path) -> io::Result<()> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;

    let now = SystemTime::now();
    let times = FileTimes::new().set_accessed(now).set_modified(now);
    file.set_times(times)?;

    Ok(())
}
