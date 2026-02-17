//! Write operations trait for archives.

use super::ArchiveRead;
use crate::BaleError;
use std::path::Path;
use std::time::SystemTime;

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
    /// - Writing to the archive fails
    fn add_entry(&mut self, path: &str, data: &[u8], mode: u32) -> Result<(), BaleError>;

    /// Adds an entry from raw data with a specific modification time.
    ///
    /// If an entry with the same path already exists, the new entry shadows it.
    /// The old data remains in the archive (orphaned) until compact.
    ///
    /// # Arguments
    ///
    /// * `path` - Archive path for the entry
    /// * `data` - File contents
    /// * `mode` - Unix file permissions (e.g., 0o644)
    /// * `mtime` - Modification time (None uses current time)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path exceeds the archive's path_size
    /// - Writing to the archive fails
    fn add_entry_with_mtime(
        &mut self,
        path: &str,
        data: &[u8],
        mode: u32,
        mtime: Option<SystemTime>,
    ) -> Result<(), BaleError>;

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
    /// - Writing to the archive fails
    fn add_file(&mut self, src: impl AsRef<Path>, archive_path: &str) -> Result<(), BaleError>;

    /// Deletes all entries matching a path.
    ///
    /// Removes all matching entries from the directory and entry tables.
    /// The file data remains in the archive (orphaned) until a compact operation.
    ///
    /// Returns `true` if any entries were deleted, `false` if none matched.
    fn delete(&mut self, path: &str) -> bool;

    /// Creates a hard link pointing to an existing entry.
    ///
    /// Adds a new directory row mapping `link` to the same entry ID as
    /// `target`. No new entry row is created — both paths share the same
    /// entry metadata and data block.
    ///
    /// # Arguments
    ///
    /// * `target` - Existing path to link to
    /// * `link` - New path for the hard link
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The target path does not exist (`EntryNotFound`)
    /// - The target is a directory (`NotAFile`)
    /// - The link path already exists (`PathExists`)
    /// - The link path exceeds path_size (`PathTooLong`)
    fn hard_link(&mut self, target: &str, link: &str) -> Result<(), BaleError>;

    /// Removes a single directory row for a path.
    ///
    /// If other directory rows still reference the same entry ID (hard links),
    /// the entry row is preserved. If no references remain, the entry row is
    /// also removed.
    ///
    /// Returns `true` if the path was found and removed, `false` otherwise.
    fn unlink(&mut self, path: &str) -> bool;

    /// Renames an entry by updating its path in the directory table.
    ///
    /// For directory entries, all descendant paths are also updated with the
    /// new prefix. The rename is atomic: if any resulting path would exceed
    /// `path_size`, no changes are made.
    ///
    /// # Arguments
    ///
    /// * `from` - Current path
    /// * `to` - New path
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The source path does not exist (`EntryNotFound`)
    /// - The destination path already exists (`PathExists`)
    /// - Any resulting path exceeds path_size (`PathTooLong`)
    fn rename(&mut self, from: &str, to: &str) -> Result<(), BaleError>;

    /// Replaces the content of an existing file entry.
    ///
    /// Writes a new data block and updates the entry row's data offset,
    /// sizes, CRC, mode, and modification time. The old data block becomes
    /// orphaned (reclaimed by compaction).
    ///
    /// # Arguments
    ///
    /// * `path` - Path of the entry to update
    /// * `data` - New file contents
    /// * `mode` - New Unix file permissions
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The entry does not exist (`EntryNotFound`)
    /// - The entry is not a file (`NotAFile`)
    /// - Writing the data block fails
    fn replace_content(&mut self, path: &str, data: &[u8], mode: u32) -> Result<(), BaleError>;

    /// Flushes all changes to disk.
    ///
    /// Rewrites the entry table, directory table, and trailer. The file is
    /// truncated to the logical size.
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
    fn add_directory(&mut self, path: impl AsRef<str>, mode: u32) -> Result<(), BaleError>;

    /// Creates a symbolic link entry.
    ///
    /// Symlink entries store the target path as their data block content,
    /// with symlink mode bits set. The target is validated to prevent
    /// absolute paths and paths that escape the archive root.
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
    /// - Writing to the archive fails
    /// - The target is an absolute path ([`BaleError::InvalidPath`])
    /// - The target escapes the archive root ([`BaleError::InvalidPath`])
    fn add_symlink(
        &mut self,
        path: impl AsRef<str>,
        target: impl AsRef<str>,
        mode: u32,
    ) -> Result<(), BaleError>;
}
