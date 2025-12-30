//! Bale archive format library.
//!
//! A mmap-first, zero-copy zip-compatible archive format with fixed-stride
//! entries for efficient random access.

/// Archive builder for creating bale archives.
mod archive;
/// Bale-specific EOCD extension.
mod bale_eocd;
/// Central Directory Header for ZIP entries.
mod central_dir;
/// MS-DOS date/time format for ZIP archives.
mod dos_time;
/// End of Central Directory record.
mod eocd;
/// Error types for bale operations.
mod error;
/// Local File Header for ZIP entries.
mod local_file;

pub use archive::Archive;
pub use bale_eocd::BaleEocd;
pub use central_dir::CentralDirectoryHeader;
pub use dos_time::DosDateTime;
pub use eocd::Eocd;
pub use error::BaleError;
pub use local_file::LocalFileHeader;

use std::fs::{FileTimes, OpenOptions};
use std::io;
use std::path::Path;
use std::time::SystemTime;

use memmap2::MmapMut;
use zerocopy::IntoBytes;

/// Creates a file or updates its modification time.
///
/// If the path has a `.bale` extension and the file doesn't exist,
/// creates a new empty bale archive. Otherwise, creates an empty file
/// or updates the modification time of an existing file.
///
/// # Errors
///
/// Returns an error if file creation or modification fails.
pub fn touch(path: impl AsRef<Path>) -> Result<(), BaleError> {
    let path = path.as_ref();
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
/// The archive contains the EOCD and BaleMetadata comment.
#[allow(unsafe_code)]
fn create_empty_archive(path: &Path) -> Result<(), BaleError> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)?;

    let total_size = BaleEocd::COMBINED_SIZE;
    file.set_len(total_size as u64)?;

    // SAFETY: We just created this file exclusively with create_new.
    let mut mmap = unsafe { MmapMut::map_mut(&file)? };

    let eocd = Eocd::new_with_comment(0, 0, 0, BaleEocd::SIZE as u16);
    let bale_eocd = BaleEocd::new();

    mmap[..Eocd::SIZE].copy_from_slice(eocd.as_bytes());
    mmap[Eocd::SIZE..].copy_from_slice(bale_eocd.as_bytes());
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
