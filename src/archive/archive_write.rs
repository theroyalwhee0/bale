//! Write operations trait for archives.

use super::ArchiveRead;
use crate::BaleError;
use std::path::Path;

/// Write operations for archives.
///
/// This trait is only implemented for `Archive<MappedArchiveMut>`.
pub trait ArchiveWrite: ArchiveRead {
    /// Adds an entry from raw data.
    ///
    /// If an entry with the same path already exists, the new entry shadows it.
    /// The old data remains in the archive (orphaned) until compact.
    ///
    /// # Arguments
    ///
    /// * `path` - Archive path for the entry
    /// * `data` - File contents
    /// * `mode` - Unix file permissions (e.g., 0o644)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path exceeds the archive's path_size
    /// - The data size exceeds 4GB (ZIP format limitation)
    /// - The archive offset would exceed 4GB (ZIP format limitation)
    /// - Writing to the archive fails
    fn add_entry(&mut self, path: &str, data: &[u8], mode: u32) -> Result<(), BaleError>;

    /// Adds a file from the filesystem to the archive.
    ///
    /// # Memory usage
    ///
    /// This method reads the entire file into memory before writing to the
    /// archive. For very large files, consider using [`add_entry()`](Self::add_entry)
    /// with a streaming approach, or ensure sufficient memory is available.
    ///
    /// # Arguments
    ///
    /// * `src` - Path to the source file
    /// * `archive_path` - Path within the archive
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The source file cannot be read
    /// - The archive path exceeds path_size
    /// - The file size exceeds 4GB (ZIP format limitation)
    /// - Writing to the archive fails
    fn add_file(&mut self, src: impl AsRef<Path>, archive_path: &str) -> Result<(), BaleError>;

    /// Deletes all entries matching a path.
    ///
    /// Removes all matching entries from the Central Directory. If duplicate
    /// entries exist (from shadowing), all are removed. The file data remains
    /// in the archive (orphaned) until a compact operation.
    ///
    /// Returns `true` if any entries were deleted, `false` if none matched.
    fn delete(&mut self, path: &str) -> bool;

    /// Flushes all changes to disk.
    ///
    /// Rewrites the Central Directory and full trailer (ZIP64 EOCD, ZIP64 EOCD
    /// Locator, EOCD, and BaleEocd). The CD starts at an aligned offset for
    /// efficient mmap access. The file is truncated to the logical size.
    ///
    /// If no changes have been made since the last sync, this is a no-op.
    ///
    /// # Errors
    ///
    /// Returns an error if writing or syncing fails.
    fn sync(&mut self) -> Result<(), BaleError>;

    /// Creates an explicit directory entry.
    ///
    /// Directory entries have zero-length data and directory mode bits set.
    /// The path should not have a trailing slash; it will be added internally
    /// if needed for ZIP compatibility.
    ///
    /// # Arguments
    ///
    /// * `path` - Archive path for the directory
    /// * `mode` - Unix directory permissions (e.g., 0o755). The directory type
    ///   bits (0o040000) will be added automatically if not present.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path exceeds the archive's path_size
    /// - Writing to the archive fails
    fn add_folder(&mut self, path: impl AsRef<str>, mode: u32) -> Result<(), BaleError>;

    /// Creates a symbolic link entry.
    ///
    /// Symlink entries store the target path as their data, with symlink mode
    /// bits set in external attributes.
    ///
    /// # Arguments
    ///
    /// * `path` - Archive path for the symlink
    /// * `target` - The symlink target (what the link points to)
    /// * `mode` - Unix permissions (e.g., 0o777). The symlink type bits
    ///   (0o120000) will be added automatically if not present.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path exceeds the archive's path_size
    /// - The target length exceeds 4GB
    /// - Writing to the archive fails
    fn add_symlink(
        &mut self,
        path: impl AsRef<str>,
        target: impl AsRef<str>,
        mode: u32,
    ) -> Result<(), BaleError>;
}
