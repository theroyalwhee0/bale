//! Read-only memory-mapped archive.

use crate::BaleError;
#[allow(unused_imports)] // Required for lock_shared() extension method
use fs4::fs_std::FileExt;
use std::fs::File;
use std::path::Path;

/// A read-only memory-mapped archive file.
///
/// This struct provides zero-copy access to archive contents by memory-mapping
/// the underlying file. A shared lock is held on the file for the lifetime of
/// this struct, allowing multiple concurrent readers while preventing writers.
///
/// The lock is automatically released when this struct is dropped.
pub struct MappedArchive {
    /// The underlying file handle (kept open to maintain the lock).
    #[allow(dead_code)]
    file: File,
    /// The memory-mapped region.
    mmap: memmap2::Mmap,
}

impl MappedArchive {
    /// Opens a file and memory-maps it for reading.
    ///
    /// Acquires a shared lock on the file, allowing multiple concurrent readers
    /// while preventing exclusive writers.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be opened
    /// - The shared lock cannot be acquired
    /// - Memory mapping fails
    ///
    /// # Safety
    ///
    /// Memory mapping is inherently unsafe because the underlying file could
    /// be modified by another process while mapped. This implementation
    /// mitigates this by holding a shared lock on the file, preventing
    /// cooperative writers from modifying it during the mapping lifetime.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BaleError> {
        let file = File::open(path.as_ref())?;
        file.lock_shared()?;
        // SAFETY: We hold a shared lock on the file, preventing cooperative
        // writers from modifying it while mapped.
        #[allow(unsafe_code)]
        let mmap = unsafe { memmap2::Mmap::map(&file)? };
        Ok(Self { file, mmap })
    }

    /// Returns the length of the mapped region in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.mmap.len()
    }

    /// Returns true if the mapped region is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mmap.is_empty()
    }

    /// Returns a byte slice of the entire mapped region.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.mmap
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Opening a valid file returns a mapped archive.
    #[test]
    fn open_valid_file() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"test data").unwrap();
        let mapped = MappedArchive::open(file.path()).unwrap();
        assert_eq!(mapped.len(), 9);
        assert_eq!(mapped.as_bytes(), b"test data");
    }

    /// Opening a nonexistent file returns an error.
    #[test]
    fn open_nonexistent_file() {
        let result = MappedArchive::open("/nonexistent/path/to/file.bale");
        assert!(result.is_err());
    }

    /// Empty files can be mapped.
    #[test]
    fn open_empty_file() {
        let file = NamedTempFile::new().unwrap();
        let mapped = MappedArchive::open(file.path()).unwrap();
        assert!(mapped.is_empty());
        assert_eq!(mapped.len(), 0);
    }
}
