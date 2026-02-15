//! Read-write memory-mapped archive.

use crate::BaleError;
use std::fs::File;
use std::path::Path;

/// A read-write memory-mapped archive file.
///
/// This struct provides mutable access to archive contents via memory-mapping.
/// An exclusive lock is held on the file for the lifetime of this struct,
/// preventing other readers and writers. The lock is automatically released
/// when this struct is dropped.
///
/// # Automatic sync on drop
///
/// The file may be pre-allocated beyond the logical content length. When this
/// struct is dropped, [`sync()`](Self::sync) is called automatically to truncate
/// the file to its committed length. However, any errors during sync are silently
/// ignored since `Drop` cannot propagate errors.
///
/// For proper error handling, call [`sync()`](Self::sync) explicitly before
/// dropping and handle the `Result`.
///
/// # Logical vs committed length
///
/// This struct tracks two lengths:
/// - **Logical length** ([`len()`](Self::len)): The current write position, used
///   by [`extend()`](Self::extend) and [`as_bytes()`](Self::as_bytes).
/// - **Committed length**: The file size to preserve on disk, set by
///   [`sync()`](Self::sync). On drop, the file is truncated to
///   `max(len, committed_len)` to preserve synced content even if the logical
///   length was later reduced for overwriting.
pub struct MappedArchiveMut {
    /// The underlying file handle (kept open to maintain the lock).
    file: File,
    /// The memory-mapped region.
    mmap: memmap2::MmapMut,
    /// Current logical length (may be less than capacity due to pre-allocation).
    len: usize,
    /// Committed file length to preserve on drop.
    committed_len: usize,
}

impl MappedArchiveMut {
    /// Default pre-allocation chunk size (1MB).
    pub const DEFAULT_CHUNK_SIZE: usize = 1024 * 1024;

    /// Creates a new empty archive file with pre-allocated space.
    ///
    /// Fails if the file already exists to prevent accidental overwrites.
    /// Use [`open()`](Self::open) to modify an existing file.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file already exists
    /// - The file cannot be created
    /// - The exclusive lock cannot be acquired
    /// - Memory mapping fails
    pub fn create(path: impl AsRef<Path>) -> Result<Self, BaleError> {
        Self::create_with_capacity(path, Self::DEFAULT_CHUNK_SIZE)
    }

    /// Creates a new empty archive file with specified initial capacity.
    ///
    /// Fails if the file already exists to prevent accidental overwrites.
    /// Use [`open()`](Self::open) to modify an existing file.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file already exists
    /// - The file cannot be created
    /// - The exclusive lock cannot be acquired
    /// - Memory mapping fails
    pub fn create_with_capacity(
        path: impl AsRef<Path>,
        capacity: usize,
    ) -> Result<Self, BaleError> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path.as_ref())?;
        file.try_lock().map_err(|e| match e {
            std::fs::TryLockError::WouldBlock => BaleError::FileLocked,
            std::fs::TryLockError::Error(io_err) => BaleError::Io(io_err),
        })?;
        file.set_len(capacity as u64)?;
        // SAFETY: We hold an exclusive lock on the file.
        #[allow(unsafe_code)]
        let mmap = unsafe { memmap2::MmapMut::map_mut(&file)? };
        Ok(Self {
            file,
            mmap,
            len: 0,
            committed_len: 0,
        })
    }

    /// Opens an existing archive file for read-write access.
    ///
    /// The logical length is set to the current file size. If a previous session
    /// called [`reserve()`](Self::reserve) but crashed before [`sync()`](Self::sync),
    /// the file may contain pre-allocated zeros beyond the actual content. The
    /// caller is responsible for validating content or using a higher-level API
    /// (like `ArchiveWriter`) that stores the logical length in the archive trailer.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be opened
    /// - The exclusive lock cannot be acquired
    /// - Memory mapping fails
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BaleError> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.as_ref())?;
        file.try_lock().map_err(|e| match e {
            std::fs::TryLockError::WouldBlock => BaleError::FileLocked,
            std::fs::TryLockError::Error(io_err) => BaleError::Io(io_err),
        })?;
        let len = file.metadata()?.len() as usize;
        // SAFETY: We hold an exclusive lock on the file.
        #[allow(unsafe_code)]
        let mmap = unsafe { memmap2::MmapMut::map_mut(&file)? };
        Ok(Self {
            file,
            mmap,
            len,
            committed_len: len,
        })
    }

    /// Returns the logical length of the archive in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns true if the archive is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the current capacity (file size) in bytes.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.mmap.len()
    }

    /// Returns a byte slice of the logical content.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.mmap[..self.len]
    }

    /// Returns a mutable byte slice of the logical content.
    #[must_use]
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        &mut self.mmap[..self.len]
    }

    ///Returns the underlying file.
    #[must_use]
    pub fn file(&self) -> &File {
        &self.file
    }

    /// Ensures the archive has at least `min_capacity` bytes available.
    ///
    /// Extends the file and remaps if necessary.
    ///
    /// # Errors
    ///
    /// Returns an error if extending or remapping fails.
    pub fn reserve(&mut self, additional: usize) -> Result<(), BaleError> {
        let required = self.len + additional;
        if required <= self.capacity() {
            return Ok(());
        }
        // Round up to next chunk size.
        let new_capacity = ((required / Self::DEFAULT_CHUNK_SIZE) + 1) * Self::DEFAULT_CHUNK_SIZE;
        log::trace!(
            "MappedArchiveMut::reserve: len={}, additional={}, capacity={} -> {}",
            self.len,
            additional,
            self.capacity(),
            new_capacity
        );
        self.resize_file(new_capacity)
    }

    /// Resizes the underlying file and remaps.
    ///
    /// This only changes the file capacity, not the logical length (`len`).
    /// The caller (typically `reserve`) is responsible for managing `len`.
    fn resize_file(&mut self, new_capacity: usize) -> Result<(), BaleError> {
        // Flush the current content before resizing.
        self.mmap.flush_async_range(0, self.len)?;
        self.file.set_len(new_capacity as u64)?;
        // SAFETY: We hold an exclusive lock on the file.
        #[allow(unsafe_code)]
        let new_mmap = unsafe { memmap2::MmapMut::map_mut(&self.file)? };
        self.mmap = new_mmap;
        Ok(())
    }

    /// Extends the archive by writing bytes at the current end.
    ///
    /// # Errors
    ///
    /// Returns an error if capacity cannot be reserved.
    pub fn extend(&mut self, data: &[u8]) -> Result<(), BaleError> {
        self.reserve(data.len())?;
        self.mmap[self.len..self.len + data.len()].copy_from_slice(data);
        self.len += data.len();
        Ok(())
    }

    /// Sets the logical length, truncating or extending with zeros.
    ///
    /// # Errors
    ///
    /// Returns an error if extending beyond capacity and reserve fails.
    pub fn set_len(&mut self, new_len: usize) -> Result<(), BaleError> {
        if new_len > self.capacity() {
            self.reserve(new_len - self.len)?;
        }
        if new_len > self.len {
            // Zero-fill the new region.
            self.mmap[self.len..new_len].fill(0);
        }
        self.len = new_len;
        Ok(())
    }

    /// Flushes changes to disk and truncates file to logical length.
    ///
    /// The file is truncated to `len` and remapped to match the new size.
    /// Call [`set_len()`](Self::set_len) first to set the desired final size.
    ///
    /// # Errors
    ///
    /// Returns an error if flushing, truncating, or remapping fails.
    pub fn sync(&mut self) -> Result<(), BaleError> {
        log::trace!(
            "MappedArchiveMut::sync: len={}, committed={}, capacity={}",
            self.len,
            self.committed_len,
            self.capacity()
        );
        self.mmap.flush_range(0, self.len)?;
        self.file.set_len(self.len as u64)?;
        self.committed_len = self.len;
        // Remap to match new file size so capacity() is accurate.
        // SAFETY: We hold an exclusive lock on the file.
        #[allow(unsafe_code)]
        let new_mmap = unsafe { memmap2::MmapMut::map_mut(&self.file)? };
        self.mmap = new_mmap;
        Ok(())
    }
}

impl Drop for MappedArchiveMut {
    /// Flushes and truncates on drop, preserving synced content.
    ///
    /// Truncates to `max(len, committed_len)` to preserve any previously
    /// synced content even if `len` was reduced for subsequent writes.
    /// For proper error handling, call [`sync()`](Self::sync) explicitly.
    fn drop(&mut self) {
        // Preserve the larger of logical length and committed length.
        // This handles the case where len was reset after sync() for
        // subsequent writes (e.g., ArchiveWriter resets len to write_offset).
        let final_len = self.len.max(self.committed_len);
        log::trace!(
            "MappedArchiveMut::drop: len={}, committed={}, final={}",
            self.len,
            self.committed_len,
            final_len
        );
        // Use async flush to avoid blocking in signal handlers.
        if let Err(e) = self.mmap.flush_async_range(0, final_len) {
            log::error!("MappedArchiveMut::drop: flush failed: {e}");
            return;
        }
        if let Err(e) = self.file.set_len(final_len as u64) {
            log::error!("MappedArchiveMut::drop: set_len failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Creating a new mutable archive allocates initial capacity.
    #[test]
    fn create_mut_archive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.bale");
        let archive = MappedArchiveMut::create(&path).unwrap();
        assert!(archive.is_empty());
        assert_eq!(archive.len(), 0);
        assert_eq!(archive.capacity(), MappedArchiveMut::DEFAULT_CHUNK_SIZE);
    }

    /// Extending a mutable archive appends data.
    #[test]
    fn extend_mut_archive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.bale");
        let mut archive = MappedArchiveMut::create(&path).unwrap();
        archive.extend(b"hello").unwrap();
        assert_eq!(archive.len(), 5);
        assert_eq!(archive.as_bytes(), b"hello");
        archive.extend(b" world").unwrap();
        assert_eq!(archive.len(), 11);
        assert_eq!(archive.as_bytes(), b"hello world");
    }

    /// Opening an existing file for mutation preserves content.
    #[test]
    fn open_mut_existing() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"existing data").unwrap();
        file.flush().unwrap();
        let archive = MappedArchiveMut::open(file.path()).unwrap();
        assert_eq!(archive.len(), 13);
        assert_eq!(archive.as_bytes(), b"existing data");
    }

    /// Reserve expands capacity when needed.
    #[test]
    fn reserve_expands_capacity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.bale");
        let mut archive = MappedArchiveMut::create_with_capacity(&path, 100).unwrap();
        assert_eq!(archive.capacity(), 100);
        archive.reserve(200).unwrap();
        assert!(archive.capacity() >= 200);
    }

    /// Sync truncates file to logical length.
    #[test]
    fn sync_truncates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.bale");
        {
            let mut archive = MappedArchiveMut::create(&path).unwrap();
            archive.extend(b"test data").unwrap();
            archive.sync().unwrap();
        }
        // Re-open and verify size.
        let metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(metadata.len(), 9);
    }

    /// Opening a file that is exclusively locked returns `FileLocked`.
    #[test]
    fn open_while_exclusively_locked() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"test data").unwrap();
        file.flush().unwrap();
        let _first = MappedArchiveMut::open(file.path()).unwrap();
        let result = MappedArchiveMut::open(file.path());
        assert!(matches!(result, Err(BaleError::FileLocked)));
    }
}
